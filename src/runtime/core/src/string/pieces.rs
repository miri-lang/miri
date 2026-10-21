// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The pieces of a string, one at a time: what `pieces` and `lines` walk.
//!
//! `split` builds a list of every piece at once, so the whole of a large text
//! is held twice. These functions let the stdlib walk the pieces from a byte
//! cursor instead: count them once, find where the piece at a byte offset
//! ends, and build that one piece. A piece starts at byte 0 or just past a
//! separator, so every offset the walk hands back lies on a character
//! boundary; an offset that does not is clamped rather than trusted.
//!
//! The pieces are exactly `split`'s: an empty separator yields the characters,
//! and a text without the separator is one piece. Lines are the pieces between
//! `"\n"`s, less one `"\r"` before the `"\n"`, and a text's final `"\n"` ends
//! its last line rather than starting an empty one.

use super::{deref_as_str, new_string_of_ascii, MiriString};
use crate::guard;

/// The text and the separator behind two FFI pointers, null reading as empty.
///
/// # Safety
/// Each pointer is null or a valid `MiriString`.
unsafe fn text_and_separator<'a>(
    s: *const MiriString,
    separator: *const MiriString,
) -> (&'a str, &'a str) {
    guard::guard_check(s as *mut u8);
    guard::guard_check(separator as *mut u8);
    (deref_as_str(s), deref_as_str(separator))
}

/// `offset` as a byte position in `text`: within it, and on a character
/// boundary, moved forward to the next boundary when it is not.
fn boundary_at(text: &str, offset: i64) -> usize {
    let mut position = offset.clamp(0, text.len() as i64) as usize;
    while !text.is_char_boundary(position) {
        position += 1;
    }
    position
}

/// How many pieces `split` would cut `s` into at `separator`.
///
/// An empty separator cuts between characters, so an empty text has none;
/// any other separator leaves one more piece than it has occurrences.
///
/// # Safety
/// `s` and `separator` are each null or a valid `MiriString`.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_piece_count(
    s: *const MiriString,
    separator: *const MiriString,
) -> i64 {
    let (text, sep) = text_and_separator(s, separator);
    let count = match sep.as_bytes() {
        [] => text.chars().count(),
        [byte] => text.bytes().filter(|b| b == byte).count() + 1,
        _ => text.matches(sep).count() + 1,
    };
    count as i64
}

/// How many lines `s` holds: one per `"\n"`, and one more for text after the
/// last `"\n"`. An empty text has no lines.
///
/// # Safety
/// `s` is null or a valid `MiriString`.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_line_count(s: *const MiriString) -> i64 {
    guard::guard_check(s as *mut u8);
    let text = deref_as_str(s).as_bytes();
    let newlines = text.iter().filter(|&&b| b == b'\n').count();
    let unterminated = text.last().is_some_and(|&b| b != b'\n');
    (newlines + usize::from(unterminated)) as i64
}

/// The byte offset where the piece of `s` starting at byte `start` ends: the
/// next occurrence of `separator` at or after `start`, or the end of `s`.
/// With an empty separator a piece is one character.
///
/// # Safety
/// `s` and `separator` are each null or a valid `MiriString`.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_piece_end(
    s: *const MiriString,
    separator: *const MiriString,
    start: i64,
) -> i64 {
    let (text, sep) = text_and_separator(s, separator);
    let start = boundary_at(text, start);
    let rest = &text[start..];
    let length = match sep.as_bytes() {
        [] => rest.chars().next().map_or(0, char::len_utf8),
        [byte] => rest.bytes().position(|b| b == *byte).unwrap_or(rest.len()),
        _ => rest.find(sep).unwrap_or(rest.len()),
    };
    (start + length) as i64
}

/// The bytes of `s` from `start` to `end`, each clamped into `s` and onto a
/// character boundary, and whether `s` is known to be ASCII.
///
/// # Safety
/// `s` is null or a valid `MiriString`.
unsafe fn slice_of<'a>(s: *const MiriString, start: i64, end: i64) -> (&'a str, bool) {
    guard::guard_check(s as *mut u8);
    let text = deref_as_str(s);
    let start = boundary_at(text, start);
    let end = boundary_at(text, end).max(start);
    // Every piece of an ASCII string is ASCII, so none is scanned again.
    let ascii = s.is_null() || (*s).ascii;
    (&text[start..end], ascii)
}

/// A new string, RC = 1, of the bytes of `s` from `start` to `end`.
///
/// # Safety
/// `s` is null or a valid `MiriString`.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_slice_bytes(
    s: *const MiriString,
    start: i64,
    end: i64,
) -> *mut MiriString {
    let (piece, ascii) = slice_of(s, start, end);
    new_string_of_ascii(piece, ascii || piece.is_ascii())
}

/// As [`miri_rt_string_slice_bytes`], for the line that ends at `end`: when a
/// `"\n"` is at `end`, one `"\r"` just before it is not part of the line. A
/// `"\r"` ending the text, with no `"\n"` after it, stays.
///
/// # Safety
/// `s` is null or a valid `MiriString`.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_line_text(
    s: *const MiriString,
    start: i64,
    end: i64,
) -> *mut MiriString {
    let (line, ascii) = slice_of(s, start, end);
    let newline_follows = usize::try_from(end)
        .ok()
        .and_then(|end| deref_as_str(s).as_bytes().get(end))
        == Some(&b'\n');
    let line = match line.strip_suffix('\r') {
        Some(stripped) if newline_follows => stripped,
        _ => line,
    };
    new_string_of_ascii(line, ascii || line.is_ascii())
}

/// `index` when it is a position among `count` pieces; otherwise ends the
/// program with the index-out-of-bounds error list indexing reports.
#[no_mangle]
pub extern "C" fn miri_rt_string_piece_index(index: i64, count: i64) -> i64 {
    if (0..count).contains(&index) {
        return index;
    }
    // SAFETY: the call only reports the two numbers and ends the process.
    unsafe { crate::array::ffi::miri_rt_array_panic_oob(index, count.max(0) as usize) }
}
