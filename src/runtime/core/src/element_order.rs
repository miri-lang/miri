// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Ordering the elements of a type-erased container.
//!
//! The runtime knows an element only by its size, so the compiler tells it
//! which rule orders two of them:
//!
//! - an element whose bytes are a signed integer or a boolean is ordered by
//!   reading those bytes as a signed number;
//! - an element whose bytes are an unsigned integer is ordered by reading them
//!   as an unsigned number, so a value with its top bit set sorts above every
//!   smaller one rather than below zero;
//! - an element whose bytes are a float is ordered by its IEEE-754 value, which
//!   the signed reading of the same bytes gets wrong for every pair of
//!   negatives, since a float stores its sign apart from its magnitude;
//! - an element whose bytes are a reference to a value carries no order in
//!   them at all; the compiler registers a comparator for those, generated
//!   from the element type's own `compare`, and the container calls through it.
//!
//! Without a comparator the last kind would be ordered by the addresses its
//! elements happen to hold, which is allocation order wearing a sort's name.

use std::cmp::Ordering;

/// A comparator over two element values, as the compiler generates it.
///
/// Each argument is the value the element slot holds — for a managed element
/// that is the pointer to the value, matching what the drop and clone callbacks
/// are handed. Returns a negative number when the first sorts before the
/// second, zero when neither does, and a positive number otherwise.
pub type ElementCompareFn = unsafe extern "C" fn(*const u8, *const u8) -> isize;

/// The element's bytes are a signed integer or a boolean.
pub const BY_SIGNED_VALUE: usize = 0;

/// The element's bytes are an unsigned integer.
pub const BY_UNSIGNED_VALUE: usize = 1;

/// The element's bytes are an IEEE-754 float.
pub const BY_FLOAT_VALUE: usize = 2;

/// How a container decides which of two of its elements sorts first.
///
/// Every container starts [`BY_SIGNED_VALUE`] with no comparator; the compiler
/// registers the rule the element type needs when it creates the container.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct ElementOrder {
    /// [`BY_SIGNED_VALUE`], [`BY_UNSIGNED_VALUE`] or [`BY_FLOAT_VALUE`];
    /// consulted when `compare_fn` is zero.
    kind: usize,
    /// Address of an [`ElementCompareFn`], or zero when the element's bytes
    /// are its value.
    compare_fn: usize,
}

impl ElementOrder {
    /// The order every new container starts with: signed values.
    pub(crate) const SIGNED: Self = Self {
        kind: BY_SIGNED_VALUE,
        compare_fn: 0,
    };

    /// Selects the rule that reads an element's bytes as its value. A kind
    /// this runtime does not know leaves the rule as it was.
    pub(crate) fn set_kind(&mut self, kind: usize) {
        if matches!(kind, BY_SIGNED_VALUE | BY_UNSIGNED_VALUE | BY_FLOAT_VALUE) {
            self.kind = kind;
        }
    }

    /// Routes ordering through the element type's own `compare`.
    pub(crate) fn set_compare_fn(&mut self, compare_fn: usize) {
        self.compare_fn = compare_fn;
    }

    /// True when the element stored at `left` sorts strictly after the one at
    /// `right`, which is what an insertion sort asks to decide whether to shift.
    unsafe fn sorts_after(&self, left: *const u8, right: *const u8, elem_size: usize) -> bool {
        if self.compare_fn != 0 {
            return self.sorts_after_through_callback(left, right);
        }
        let ordering = match self.kind {
            BY_UNSIGNED_VALUE => compare_unsigned(left, right, elem_size),
            BY_FLOAT_VALUE => compare_floats(left, right, elem_size),
            _ => compare_signed(left, right, elem_size),
        };
        ordering == Ordering::Greater
    }

    /// Asks the element type's `compare` about the two values the slots hold.
    unsafe fn sorts_after_through_callback(&self, left: *const u8, right: *const u8) -> bool {
        let compare: ElementCompareFn = std::mem::transmute(self.compare_fn);
        let left_value = (left as *const *const u8).read_unaligned();
        let right_value = (right as *const *const u8).read_unaligned();
        compare(left_value, right_value) > 0
    }
}

/// Orders two signed integer elements by value.
///
/// A sixteen-byte slot is compared at its own width; reading it into a 64-bit
/// word would order two values by their low eight bytes and call any pair that
/// agrees there equal.
unsafe fn compare_signed(left: *const u8, right: *const u8, elem_size: usize) -> Ordering {
    if elem_size == WIDE_ELEMENT_BYTES {
        return (left as *const i128)
            .read_unaligned()
            .cmp(&(right as *const i128).read_unaligned());
    }
    read_signed(left, elem_size).cmp(&read_signed(right, elem_size))
}

/// Orders two unsigned integer elements by value.
///
/// Compared at sixteen bytes for the same reason as the signed reading, and as
/// `u128` so a value with the high bit set is the largest rather than the
/// smallest.
unsafe fn compare_unsigned(left: *const u8, right: *const u8, elem_size: usize) -> Ordering {
    if elem_size == WIDE_ELEMENT_BYTES {
        return (left as *const u128)
            .read_unaligned()
            .cmp(&(right as *const u128).read_unaligned());
    }
    read_unsigned(left, elem_size).cmp(&read_unsigned(right, elem_size))
}

/// The width of an element that holds a 128-bit scalar.
const WIDE_ELEMENT_BYTES: usize = 16;

/// Reads an element's bytes as a signed 64-bit integer, sign-extending the
/// common widths. Any other width is zero-padded.
unsafe fn read_signed(ptr: *const u8, elem_size: usize) -> i64 {
    match elem_size {
        1 => *(ptr as *const i8) as i64,
        2 => (ptr as *const i16).read_unaligned() as i64,
        4 => (ptr as *const i32).read_unaligned() as i64,
        8 => (ptr as *const i64).read_unaligned(),
        _ => i64::from_ne_bytes(padded_word(ptr, elem_size)),
    }
}

/// Reads an element's bytes as an unsigned 64-bit integer, zero-extending the
/// common widths. Any other width is zero-padded.
unsafe fn read_unsigned(ptr: *const u8, elem_size: usize) -> u64 {
    match elem_size {
        1 => *ptr as u64,
        2 => (ptr as *const u16).read_unaligned() as u64,
        4 => (ptr as *const u32).read_unaligned() as u64,
        8 => (ptr as *const u64).read_unaligned(),
        _ => u64::from_ne_bytes(padded_word(ptr, elem_size)),
    }
}

/// The first eight bytes of an element, zero-padded when it is narrower.
///
/// Reached only by a width that is neither a CPU integer width nor the
/// sixteen-byte slot, both of which are read at their own width before here.
unsafe fn padded_word(ptr: *const u8, elem_size: usize) -> [u8; 8] {
    let mut buf = [0u8; 8];
    std::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), elem_size.min(8));
    buf
}

/// Orders two float elements by value.
///
/// Uses the IEEE-754 total order, so the result is defined for every pair:
/// `-0.0` sorts before `0.0`, and a NaN sorts after every number (or before
/// every one, when its sign bit is set). A width that is not a CPU float falls
/// back to the signed reading.
unsafe fn compare_floats(left: *const u8, right: *const u8, elem_size: usize) -> Ordering {
    match elem_size {
        4 => {
            let l = (left as *const f32).read_unaligned();
            l.total_cmp(&(right as *const f32).read_unaligned())
        }
        8 => {
            let l = (left as *const f64).read_unaligned();
            l.total_cmp(&(right as *const f64).read_unaligned())
        }
        _ => read_signed(left, elem_size).cmp(&read_signed(right, elem_size)),
    }
}

/// Sorts `len` elements of `elem_size` bytes each, in place, ascending by
/// `order`.
///
/// Insertion sort: stable, so elements that compare equal keep the order they
/// were given in, and quick on the short collections a compiled program sorts
/// most often.
///
/// # Safety
///
/// `data` must point at `len * elem_size` initialized, writable bytes, and a
/// comparator registered on `order` must accept the values those elements hold.
pub(crate) unsafe fn sort_elements(
    data: *mut u8,
    len: usize,
    elem_size: usize,
    order: ElementOrder,
) {
    if data.is_null() || len < 2 || elem_size == 0 {
        return;
    }
    let mut key = vec![0u8; elem_size];

    for i in 1..len {
        std::ptr::copy_nonoverlapping(data.add(i * elem_size), key.as_mut_ptr(), elem_size);

        let mut j = i;
        while j > 0 {
            let previous = data.add((j - 1) * elem_size);
            if !order.sorts_after(previous, key.as_ptr(), elem_size) {
                break;
            }
            std::ptr::copy_nonoverlapping(previous, data.add(j * elem_size), elem_size);
            j -= 1;
        }
        std::ptr::copy_nonoverlapping(key.as_ptr(), data.add(j * elem_size), elem_size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Orders two element values by the second byte of what each points at, so
    /// a test can tell a comparator call apart from a read of the slot itself.
    unsafe extern "C" fn compare_second_byte(a: *const u8, b: *const u8) -> isize {
        (*a.add(1) as isize) - (*b.add(1) as isize)
    }

    fn ordered_through(compare: ElementCompareFn) -> ElementOrder {
        let mut order = ElementOrder::SIGNED;
        order.set_compare_fn(compare as usize);
        order
    }

    fn ordered_by(kind: usize) -> ElementOrder {
        let mut order = ElementOrder::SIGNED;
        order.set_kind(kind);
        order
    }

    #[test]
    fn without_a_comparator_elements_are_ordered_by_their_bytes() {
        let mut elements: Vec<i64> = vec![30, -10, 20];
        unsafe {
            sort_elements(elements.as_mut_ptr() as *mut u8, 3, 8, ElementOrder::SIGNED);
        }
        assert_eq!(elements, vec![-10, 20, 30]);
    }

    #[test]
    fn a_comparator_decides_the_order_instead_of_the_slot_bytes() {
        // Three values laid out so that ordering by the slot (the address) and
        // ordering by the pointed-at byte disagree: the slots ascend, the
        // second bytes descend.
        let values: Vec<[u8; 2]> = vec![[0, 3], [0, 2], [0, 1]];
        let mut slots: Vec<*const u8> = values.iter().map(|v| v.as_ptr()).collect();
        slots.sort_unstable();
        let ascending_slots = slots.clone();

        unsafe {
            sort_elements(
                slots.as_mut_ptr() as *mut u8,
                3,
                std::mem::size_of::<*const u8>(),
                ordered_through(compare_second_byte),
            );
        }

        let seconds: Vec<u8> = slots.iter().map(|p| unsafe { *p.add(1) }).collect();
        assert_eq!(seconds, vec![1, 2, 3]);
        assert_ne!(slots, ascending_slots);
    }

    #[test]
    fn equal_elements_keep_the_order_they_were_given_in() {
        let first: [u8; 2] = [7, 1];
        let second: [u8; 2] = [9, 1];
        let mut slots: Vec<*const u8> = vec![first.as_ptr(), second.as_ptr()];
        unsafe {
            sort_elements(
                slots.as_mut_ptr() as *mut u8,
                2,
                std::mem::size_of::<*const u8>(),
                ordered_through(compare_second_byte),
            );
        }
        assert_eq!(unsafe { *slots[0] }, 7);
        assert_eq!(unsafe { *slots[1] }, 9);
    }

    #[test]
    fn a_collection_of_fewer_than_two_elements_is_left_alone() {
        let mut one: Vec<i64> = vec![5];
        unsafe {
            sort_elements(one.as_mut_ptr() as *mut u8, 1, 8, ElementOrder::SIGNED);
            sort_elements(std::ptr::null_mut(), 4, 8, ElementOrder::SIGNED);
        }
        assert_eq!(one, vec![5]);
    }

    #[test]
    fn unsigned_elements_with_the_top_bit_set_sort_above_smaller_ones() {
        let mut wide: Vec<u64> = vec![u64::MAX, 1, 1 << 63];
        let mut narrow: Vec<u32> = vec![u32::MAX, 3, 1 << 31];
        let mut byte: Vec<u8> = vec![200, 7];
        unsafe {
            sort_elements(
                wide.as_mut_ptr() as *mut u8,
                3,
                8,
                ordered_by(BY_UNSIGNED_VALUE),
            );
            sort_elements(
                narrow.as_mut_ptr() as *mut u8,
                3,
                4,
                ordered_by(BY_UNSIGNED_VALUE),
            );
            sort_elements(byte.as_mut_ptr(), 2, 1, ordered_by(BY_UNSIGNED_VALUE));
        }
        assert_eq!(wide, vec![1, 1 << 63, u64::MAX]);
        assert_eq!(narrow, vec![3, 1 << 31, u32::MAX]);
        assert_eq!(byte, vec![7, 200]);
    }

    #[test]
    fn float_elements_are_ordered_by_value_including_negatives() {
        let mut wide: Vec<f64> = vec![-1.5, 0.5, -2.5, -0.25];
        let mut narrow: Vec<f32> = vec![-1.0, -3.0, 2.0];
        unsafe {
            sort_elements(
                wide.as_mut_ptr() as *mut u8,
                4,
                8,
                ordered_by(BY_FLOAT_VALUE),
            );
            sort_elements(
                narrow.as_mut_ptr() as *mut u8,
                3,
                4,
                ordered_by(BY_FLOAT_VALUE),
            );
        }
        assert_eq!(wide, vec![-2.5, -1.5, -0.25, 0.5]);
        assert_eq!(narrow, vec![-3.0, -1.0, 2.0]);
    }

    #[test]
    fn a_nan_sorts_after_every_number() {
        let mut values: Vec<f64> = vec![f64::NAN, 1.0, f64::INFINITY, -1.0];
        unsafe {
            sort_elements(
                values.as_mut_ptr() as *mut u8,
                4,
                8,
                ordered_by(BY_FLOAT_VALUE),
            );
        }
        assert_eq!(&values[..3], &[-1.0, 1.0, f64::INFINITY]);
        assert!(values[3].is_nan());
    }

    #[test]
    fn an_unknown_kind_leaves_the_signed_rule_in_place() {
        let mut elements: Vec<i64> = vec![5, -5];
        unsafe {
            sort_elements(elements.as_mut_ptr() as *mut u8, 2, 8, ordered_by(99));
        }
        assert_eq!(elements, vec![-5, 5]);
    }
}
