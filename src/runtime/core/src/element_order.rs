// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Ordering the elements of a type-erased container.
//!
//! The runtime knows an element only by its size, so it has two ways to decide
//! which of two elements sorts first. An element whose bytes *are* its value —
//! every integer, float and boolean — is ordered by reading those bytes as a
//! number. An element whose bytes are a reference to a value carries no order
//! at all in them; the compiler registers a comparator for those, generated
//! from the element type's own `compare`, and the container calls through it.
//!
//! Without a comparator the second kind would be ordered by the addresses its
//! elements happen to hold, which is allocation order wearing a sort's name.

/// A comparator over two element values, as the compiler generates it.
///
/// Each argument is the value the element slot holds — for a managed element
/// that is the pointer to the value, matching what the drop and clone callbacks
/// are handed. Returns a negative number when the first sorts before the
/// second, zero when neither does, and a positive number otherwise.
pub type ElementCompareFn = unsafe extern "C" fn(*const u8, *const u8) -> isize;

/// Reads raw bytes as a signed 64-bit integer for comparison purposes.
///
/// Handles common element sizes (1, 2, 4, 8 bytes) with sign extension.
/// Other sizes are zero-padded.
///
/// TODO: an unsigned element above `i64::MAX` reads back negative here, so a
/// `List<u64>` holding one sorts it before every smaller value. Ordering an
/// unsigned element correctly needs the container to carry the element's
/// signedness, which nothing records yet.
pub(crate) unsafe fn read_as_i64(ptr: *const u8, elem_size: usize) -> i64 {
    match elem_size {
        1 => *(ptr as *const i8) as i64,
        2 => *(ptr as *const i16) as i64,
        4 => *(ptr as *const i32) as i64,
        8 => *(ptr as *const i64),
        _ => {
            let mut buf = [0u8; 8];
            let copy_len = elem_size.min(8);
            std::ptr::copy_nonoverlapping(ptr, buf.as_mut_ptr(), copy_len);
            i64::from_ne_bytes(buf)
        }
    }
}

/// True when the element stored at `left` sorts strictly after the one at
/// `right`, which is what an insertion sort asks to decide whether to shift.
///
/// `compare_fn` is zero for an element ordered by its bytes.
unsafe fn sorts_after(
    left: *const u8,
    right: *const u8,
    elem_size: usize,
    compare_fn: usize,
) -> bool {
    if compare_fn == 0 {
        return read_as_i64(left, elem_size) > read_as_i64(right, elem_size);
    }
    let compare: ElementCompareFn = std::mem::transmute(compare_fn);
    let left_value = *(left as *const *const u8);
    let right_value = *(right as *const *const u8);
    compare(left_value, right_value) > 0
}

/// Sorts `len` elements of `elem_size` bytes each, in place, ascending.
///
/// Insertion sort: stable, so elements that compare equal keep the order they
/// were given in, and quick on the short collections a compiled program sorts
/// most often.
///
/// # Safety
///
/// `data` must point at `len * elem_size` initialized, writable bytes, and
/// `compare_fn` must be zero or the address of an [`ElementCompareFn`] that
/// accepts the values those elements hold.
pub unsafe fn sort_elements(data: *mut u8, len: usize, elem_size: usize, compare_fn: usize) {
    if data.is_null() || len < 2 || elem_size == 0 {
        return;
    }
    let mut key = vec![0u8; elem_size];

    for i in 1..len {
        std::ptr::copy_nonoverlapping(data.add(i * elem_size), key.as_mut_ptr(), elem_size);

        let mut j = i;
        while j > 0 {
            let previous = data.add((j - 1) * elem_size);
            if !sorts_after(previous, key.as_ptr(), elem_size, compare_fn) {
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

    #[test]
    fn without_a_comparator_elements_are_ordered_by_their_bytes() {
        let mut elements: Vec<i64> = vec![30, -10, 20];
        unsafe {
            sort_elements(elements.as_mut_ptr() as *mut u8, 3, 8, 0);
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
                compare_second_byte as usize,
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
                compare_second_byte as usize,
            );
        }
        assert_eq!(unsafe { *slots[0] }, 7);
        assert_eq!(unsafe { *slots[1] }, 9);
    }

    #[test]
    fn a_collection_of_fewer_than_two_elements_is_left_alone() {
        let mut one: Vec<i64> = vec![5];
        unsafe {
            sort_elements(one.as_mut_ptr() as *mut u8, 1, 8, 0);
            sort_elements(std::ptr::null_mut(), 4, 8, 0);
        }
        assert_eq!(one, vec![5]);
    }
}
