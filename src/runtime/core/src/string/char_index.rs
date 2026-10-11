// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Character positions in a string without a rescan per step.
//!
//! UTF-8 is variable-width, so "the character at index i" and "how many
//! characters" both cost a scan from the start. A `for ch in s` loop asks for
//! the length and then character 0, 1, 2, ... — a scan per step makes the
//! whole walk quadratic. This module remembers, for the few strings walked
//! most recently on this thread, the character count and the byte offset of
//! the last character found, and answers the next question from there:
//!
//! - once the count is known and equals the byte length, the string is ASCII
//!   and a character index *is* a byte offset;
//! - otherwise a lookup at or past the remembered character resumes the scan
//!   from its byte offset, so a front-to-back walk steps over each character
//!   once in total.
//!
//! The memory is sound because a string's bytes never change while it is
//! alive: every operation returns a new string. An entry is keyed by the
//! string's address, its data address and its byte length, and dropping a
//! string forgets its entry, so a later string allocated at the same address
//! never inherits it.

use std::cell::Cell;

use super::MiriString;

/// How many strings keep their positions at once. A loop that walks two
/// strings side by side keeps both.
const ENTRIES: usize = 4;

/// What is remembered about one string.
#[derive(Clone, Copy)]
struct Entry {
    string: *const MiriString,
    data: *const u8,
    len: usize,
    char_count: Option<usize>,
    /// The character last found, and the byte offset it starts at.
    char_index: usize,
    byte_offset: usize,
}

impl Entry {
    fn fresh(string: &MiriString) -> Self {
        Self {
            string,
            data: string.data,
            len: string.len,
            char_count: None,
            char_index: 0,
            byte_offset: 0,
        }
    }

    fn describes(&self, string: &MiriString) -> bool {
        std::ptr::eq(self.string, string)
            && std::ptr::eq(self.data, string.data)
            && self.len == string.len
    }

    /// True when every character is one byte, so indexes are byte offsets.
    fn is_ascii(&self) -> bool {
        self.char_count == Some(self.len)
    }
}

#[derive(Clone, Copy)]
struct Cache {
    entries: [Option<Entry>; ENTRIES],
    /// The slot the next new string replaces.
    next: usize,
}

thread_local! {
    static CACHE: Cell<Cache> = const {
        Cell::new(Cache { entries: [None; ENTRIES], next: 0 })
    };
}

/// Runs `f` on the entry for `string`, creating one (and evicting the oldest
/// string) when there is none. While the thread is being torn down there is
/// no cache, and `f` runs on a fresh entry that is not kept.
fn with_entry<R>(string: &MiriString, f: impl FnOnce(&mut Entry) -> R) -> R {
    let Ok(mut cache) = CACHE.try_with(Cell::get) else {
        return f(&mut Entry::fresh(string));
    };
    let found = cache.entries.iter().enumerate().find_map(|(slot, entry)| {
        entry
            .filter(|entry| entry.describes(string))
            .map(|entry| (slot, entry))
    });
    let (slot, mut entry) = found.unwrap_or_else(|| {
        let slot = cache.next;
        cache.next = (slot + 1) % ENTRIES;
        (slot, Entry::fresh(string))
    });
    let result = f(&mut entry);
    cache.entries[slot] = Some(entry);
    // Teardown between the read and this write only loses the entry.
    let _ = CACHE.try_with(|cell| cell.set(cache));
    result
}

/// Forgets whatever is remembered about `string`. Called as it is dropped.
pub(crate) fn forget(string: &MiriString) {
    let _ = CACHE.try_with(|cell| {
        let mut cache = cell.get();
        for entry in &mut cache.entries {
            if entry.is_some_and(|entry| std::ptr::eq(entry.string, string)) {
                *entry = None;
            }
        }
        cell.set(cache);
    });
}

/// The number of characters in `string`, counted once per string.
///
/// # Safety
/// `string` holds valid UTF-8.
pub(crate) unsafe fn char_count(string: &MiriString) -> usize {
    let text = string.as_str();
    with_entry(string, |entry| {
        *entry.char_count.get_or_insert_with(|| {
            count_probes(text.len());
            text.chars().count()
        })
    })
}

/// The byte offset where character `index` of `string` starts, or `None`
/// when the string has `index` characters or fewer.
///
/// # Safety
/// `string` holds valid UTF-8.
pub(crate) unsafe fn char_byte_offset(string: &MiriString, index: usize) -> Option<usize> {
    let text = string.as_str();
    with_entry(string, |entry| {
        if entry.is_ascii() {
            return (index < entry.len).then_some(index);
        }
        if entry.char_count.is_some_and(|count| index >= count) {
            return None;
        }
        let (from_index, from_offset) = if entry.char_index <= index {
            (entry.char_index, entry.byte_offset)
        } else {
            (0, 0)
        };
        let steps = index - from_index;
        count_probes(steps + 1);
        let (relative, _) = text.get(from_offset..)?.char_indices().nth(steps)?;
        entry.char_index = index;
        entry.byte_offset = from_offset + relative;
        Some(entry.byte_offset)
    })
}

/// The byte offset of the boundary before character `index`: where that
/// character starts, or the string's end when `index` equals its character
/// count. `None` past that.
///
/// # Safety
/// `string` holds valid UTF-8.
pub(crate) unsafe fn char_boundary(string: &MiriString, index: usize) -> Option<usize> {
    if index == char_count(string) {
        return Some(string.len);
    }
    char_byte_offset(string, index)
}

/// How many characters of `string` lie before byte offset `byte`, which must
/// be a character boundary. A run of lookups at increasing offsets, the way
/// the matches of one search arrive, steps over each character once in total.
///
/// # Safety
/// `string` holds valid UTF-8 and `byte` is a character boundary in it.
pub(crate) unsafe fn char_index_of_byte(string: &MiriString, byte: usize) -> usize {
    let text = string.as_str();
    with_entry(string, |entry| {
        if entry.is_ascii() {
            return byte;
        }
        let (from_index, from_offset) = if entry.byte_offset <= byte {
            (entry.char_index, entry.byte_offset)
        } else {
            (0, 0)
        };
        let between = text.get(from_offset..byte).unwrap_or("");
        count_probes(between.len());
        let index = from_index + between.chars().count();
        if byte < text.len() {
            entry.char_index = index;
            entry.byte_offset = byte;
        }
        index
    })
}

/// The character at `index` of `string`, or `None` past its end.
///
/// # Safety
/// `string` holds valid UTF-8.
pub(crate) unsafe fn char_at(string: &MiriString, index: usize) -> Option<char> {
    let offset = char_byte_offset(string, index)?;
    string.as_str().get(offset..)?.chars().next()
}

#[cfg(test)]
thread_local! {
    static PROBES: Cell<usize> = const { Cell::new(0) };
}

/// Counts bytes or characters stepped over to answer a lookup, for the tests
/// that hold a full walk to a linear cost.
#[inline]
fn count_probes(_steps: usize) {
    #[cfg(test)]
    PROBES.with(|probes| probes.set(probes.get() + _steps));
}

/// The steps taken by lookups on this thread since the last call, resetting
/// the count.
#[cfg(test)]
pub(crate) fn take_probes() -> usize {
    PROBES.with(|probes| probes.replace(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-megabyte text of `unit` repeated.
    fn megabyte_of(unit: &str) -> MiriString {
        MiriString::from_str(&unit.repeat((1 << 20) / unit.len()))
    }

    /// Walks `string` the way `for ch in s` does: the length, then each
    /// character in turn. Returns the characters seen.
    unsafe fn walk(string: &MiriString) -> String {
        let mut seen = String::new();
        let mut index = 0;
        while index < char_count(string) {
            seen.extend(char_at(string, index));
            index += 1;
        }
        seen
    }

    #[test]
    fn ascii_walk_of_a_megabyte_is_linear() {
        let string = megabyte_of("ab cd ef g ");
        take_probes();
        let seen = unsafe { walk(&string) };
        let probes = take_probes();
        assert_eq!(seen, unsafe { string.as_str() });
        assert!(
            probes <= 2 * string.len,
            "walk of {} bytes probed {probes}",
            string.len
        );
    }

    #[test]
    fn multibyte_walk_of_a_megabyte_is_linear() {
        let string = megabyte_of("añ€😀 ");
        take_probes();
        let seen = unsafe { walk(&string) };
        let probes = take_probes();
        assert_eq!(seen, unsafe { string.as_str() });
        assert!(
            probes <= 3 * string.len,
            "walk of {} bytes probed {probes}",
            string.len
        );
    }

    #[test]
    fn code_walk_without_the_length_is_linear() {
        let string = megabyte_of("añ€😀 ");
        take_probes();
        let mut index = 0;
        while unsafe { char_at(&string, index) }.is_some() {
            index += 1;
        }
        let probes = take_probes();
        assert_eq!(index, unsafe { string.as_str() }.chars().count());
        assert!(
            probes <= 2 * string.len,
            "walk of {} bytes probed {probes}",
            string.len
        );
    }

    #[test]
    fn lookups_out_of_order_find_the_right_character() {
        let string = MiriString::from_str("añ€😀z");
        let expected: Vec<char> = "añ€😀z".chars().collect();
        for index in [3, 1, 4, 0, 2, 2, 5, 4] {
            assert_eq!(
                unsafe { char_at(&string, index) },
                expected.get(index).copied()
            );
        }
    }

    #[test]
    fn two_strings_walked_side_by_side_keep_their_own_positions() {
        let left = MiriString::from_str("añ€😀z");
        let right = MiriString::from_str("xyz");
        let pairs: Vec<_> = (0..3)
            .map(|index| unsafe { (char_at(&left, index), char_at(&right, index)) })
            .collect();
        assert_eq!(
            pairs,
            vec![
                (Some('a'), Some('x')),
                (Some('ñ'), Some('y')),
                (Some('€'), Some('z'))
            ]
        );
    }

    #[test]
    fn a_string_dropped_and_replaced_at_the_same_address_is_read_afresh() {
        let mut slot = std::mem::ManuallyDrop::new(MiriString::from_str("añ€"));
        assert_eq!(unsafe { char_count(&slot) }, 3);
        assert_eq!(unsafe { char_at(&slot, 2) }, Some('€'));
        unsafe { std::mem::ManuallyDrop::drop(&mut slot) };
        // Same byte length, so its data may well land where the old data was.
        let replacement = MiriString::from_str("uvwxyz");
        unsafe { std::ptr::write(&mut *slot, replacement) };
        assert_eq!(unsafe { char_count(&slot) }, 6);
        assert_eq!(unsafe { char_at(&slot, 2) }, Some('w'));
        unsafe { std::mem::ManuallyDrop::drop(&mut slot) };
    }

    #[test]
    fn byte_offsets_become_character_positions_in_one_pass() {
        let text = MiriString::from_str("a😀b😀");
        let offsets = [0, 1, 5, 6, 10];
        let positions: Vec<usize> = offsets
            .iter()
            .map(|&byte| unsafe { char_index_of_byte(&text, byte) })
            .collect();
        assert_eq!(positions, [0, 1, 2, 3, 4]);
        take_probes();
        let long = megabyte_of("é");
        for byte in (0..long.len).step_by(2) {
            unsafe { char_index_of_byte(&long, byte) };
        }
        assert!(
            take_probes() <= long.len + 2,
            "increasing offsets rescanned the string"
        );
    }

    #[test]
    fn the_boundary_after_the_last_character_is_the_end() {
        let text = MiriString::from_str("héllo");
        assert_eq!(unsafe { char_boundary(&text, 5) }, Some(6));
        assert_eq!(unsafe { char_boundary(&text, 1) }, Some(1));
        assert_eq!(unsafe { char_boundary(&text, 2) }, Some(3));
        assert_eq!(unsafe { char_boundary(&text, 6) }, None);
    }
}
