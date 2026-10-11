// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Positional access into an open-addressing table.
//!
//! A hash table answers "the nth entry" by counting occupied slots, which costs
//! a scan from slot 0. Iteration asks for entry 0, then 1, then 2, so a scan
//! per step makes a full walk quadratic. A [`SlotCursor`] remembers where the
//! last answer landed and resumes from there whenever the next question is at
//! or past it, so a front-to-back walk visits each slot once in total.
//!
//! The cursor describes the table's slot states as they were when it was
//! recorded. Whatever changes which slots are occupied — an insertion into a
//! new slot, a removal, a rehash, a clear — must [`SlotCursor::forget`] it. A
//! value written over an existing entry occupies the same slot and leaves it
//! valid.

use std::cell::Cell;

/// Where the most recent positional lookup into a table landed.
///
/// All-zero bytes are a valid, empty cursor, so a table allocated zeroed
/// starts without one.
#[repr(C)]
#[derive(Default)]
pub(crate) struct SlotCursor {
    /// One past the position of the remembered entry; zero when there is none.
    position_after: Cell<usize>,
    /// The slot holding the remembered entry.
    slot: Cell<usize>,
}

impl SlotCursor {
    /// Drops the remembered entry; the next lookup scans from slot 0.
    pub(crate) fn forget(&self) {
        self.position_after.set(0);
    }

    /// The slot of the `nth` occupied slot among `0..capacity`, or `None`
    /// when fewer than `nth + 1` slots are occupied.
    ///
    /// `is_occupied` reports one slot's state. The scan resumes from the
    /// remembered entry when `nth` is at or past it, else starts at slot 0.
    pub(crate) fn nth_occupied(
        &self,
        capacity: usize,
        nth: usize,
        is_occupied: impl Fn(usize) -> bool,
    ) -> Option<usize> {
        let (mut position, start) = match self.position_after.get() {
            0 => (0, 0),
            after if after - 1 <= nth => (after - 1, self.slot.get()),
            _ => (0, 0),
        };
        for slot in start..capacity {
            count_probe();
            if !is_occupied(slot) {
                continue;
            }
            if position == nth {
                self.position_after.set(nth + 1);
                self.slot.set(slot);
                return Some(slot);
            }
            position += 1;
        }
        None
    }
}

#[cfg(test)]
thread_local! {
    static PROBES: Cell<usize> = const { Cell::new(0) };
}

/// Counts one slot inspected by a positional lookup, for the tests that hold
/// a full walk to a linear number of probes.
#[inline]
fn count_probe() {
    #[cfg(test)]
    PROBES.with(|probes| probes.set(probes.get() + 1));
}

/// The slots inspected by positional lookups on this thread since the last
/// call, resetting the count.
#[cfg(test)]
pub(crate) fn take_probes() -> usize {
    PROBES.with(|probes| probes.replace(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table of `capacity` slots where every third slot is occupied.
    fn every_third(capacity: usize) -> impl Fn(usize) -> bool {
        move |slot| slot < capacity && slot % 3 == 0
    }

    #[test]
    fn walk_front_to_back_finds_every_entry_in_slot_order() {
        let cursor = SlotCursor::default();
        let occupied = every_third(30);
        let slots: Vec<_> = (0..10)
            .map(|nth| cursor.nth_occupied(30, nth, &occupied))
            .collect();
        let expected: Vec<_> = (0..10).map(|nth| Some(nth * 3)).collect();
        assert_eq!(slots, expected);
        assert_eq!(cursor.nth_occupied(30, 10, &occupied), None);
    }

    #[test]
    fn walk_front_to_back_probes_each_slot_once() {
        let capacity = 300_000;
        let cursor = SlotCursor::default();
        let occupied = every_third(capacity);
        take_probes();
        let mut nth = 0;
        while cursor.nth_occupied(capacity, nth, &occupied).is_some() {
            nth += 1;
        }
        assert_eq!(nth, 100_000);
        assert!(take_probes() <= capacity + nth);
    }

    #[test]
    fn lookup_behind_the_cursor_rescans_from_the_start() {
        let cursor = SlotCursor::default();
        let occupied = every_third(30);
        assert_eq!(cursor.nth_occupied(30, 7, &occupied), Some(21));
        assert_eq!(cursor.nth_occupied(30, 2, &occupied), Some(6));
        assert_eq!(cursor.nth_occupied(30, 2, &occupied), Some(6));
        assert_eq!(cursor.nth_occupied(30, 9, &occupied), Some(27));
    }

    #[test]
    fn forgotten_cursor_counts_the_changed_table_from_slot_zero() {
        let cursor = SlotCursor::default();
        assert_eq!(cursor.nth_occupied(30, 5, every_third(30)), Some(15));
        cursor.forget();
        // Every slot is now occupied: entry 5 is slot 5, not slot 15 + 0.
        assert_eq!(cursor.nth_occupied(30, 5, |_| true), Some(5));
    }
}
