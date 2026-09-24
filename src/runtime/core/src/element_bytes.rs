// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How an element handed to a container by address fills the container's slot.
//!
//! Every entry point that takes an element — to store it or to look it up —
//! receives two arguments: the address of the element's bytes and `payload`,
//! the number of those bytes that are the element. The container's slot may be
//! wider than the payload: a set or a map never allocates a slot narrower than
//! a value word, and a vector with three components is padded to the stride of
//! four. The bytes the payload does not cover are zero, at the store and at
//! every lookup, so two elements that are equal occupy identical slots.
//!
//! One case runs the other way. A body compiled once for every element type
//! (a collection method that sees its element only as a type parameter) holds
//! the element in a value word whatever its real width, so it hands over a
//! word's worth of payload. A slot narrower than the word keeps the word's low
//! bytes, which on the little-endian targets Miri supports are the element.

use std::mem::size_of;
use std::ptr;

/// The width of the value word a type-parameter element travels in.
const VALUE_WORD: usize = size_of::<usize>();

/// The widest slot [`with_slot_bytes`] pads on the stack; wider ones use the heap.
const STACK_SLOT_BYTES: usize = 64;

/// Whether an element of `payload` bytes fills a `slot`-byte slot without
/// losing or inventing any of it.
///
/// A payload no wider than the slot is the whole element. A value word into a
/// narrower slot is a type-parameter element whose real width is the slot's.
/// Anything else would drop real bytes: a value word handed where a wider
/// element belongs carries only part of it, and storing that part would make
/// the container hold a value nobody wrote.
pub fn fits_slot(payload: usize, slot: usize) -> bool {
    if payload == VALUE_WORD {
        slot <= VALUE_WORD
    } else {
        payload <= slot
    }
}

/// End the process when an element of `payload` bytes does not fit a
/// `slot`-byte slot, naming both widths.
///
/// A mismatch is a compiler defect: storing the element would keep part of it
/// or invent the rest, and looking it up would compare different bytes than
/// the store wrote. Neither is recoverable, so the process stops with a report.
pub fn require_fits_slot(payload: usize, slot: usize) {
    if !fits_slot(payload, slot) {
        eprintln!(
            "Runtime error: a collection element of {payload} bytes was handed to a slot of {slot} bytes"
        );
        std::process::abort();
    }
}

/// Copy the element at `src` into the `slot`-byte slot at `dest`, zeroing every
/// byte the element does not cover.
///
/// # Safety
/// - `src` must be readable for `payload.min(slot)` bytes.
/// - `dest` must be writable for `slot` bytes and must not overlap `src`.
pub unsafe fn write_slot(dest: *mut u8, src: *const u8, payload: usize, slot: usize) {
    let copied = payload.min(slot);
    ptr::copy_nonoverlapping(src, dest, copied);
    ptr::write_bytes(dest.add(copied), 0, slot - copied);
}

/// Copy the element in the `slot`-byte slot at `src` out to the caller's
/// `payload`-byte storage at `dest`, zeroing any byte the slot does not cover.
///
/// This is how every entry point that hands an element back delivers it: the
/// caller names where the element goes and how wide it is there, so an element
/// wider than a value word arrives whole and a narrower one arrives at its own
/// width. `src` null writes a zero element, which is what an absent element
/// reads as.
///
/// # Safety
/// - `src` is null or readable for `payload.min(slot)` bytes.
/// - `dest` must be writable for `payload` bytes and must not overlap `src`.
pub unsafe fn read_slot(dest: *mut u8, src: *const u8, payload: usize, slot: usize) {
    if dest.is_null() {
        return;
    }
    let copied = if src.is_null() {
        0
    } else {
        require_fits_slot(payload, slot);
        payload.min(slot)
    };
    if copied > 0 {
        ptr::copy_nonoverlapping(src, dest, copied);
    }
    ptr::write_bytes(dest.add(copied), 0, payload - copied);
}

/// Run `f` over the element at `src` laid out at the full width of a
/// `slot`-byte slot.
///
/// A container compares and copies whole slots, so an element narrower than
/// its slot is first padded into a scratch slot the way [`write_slot`] would
/// store it. An element that already fills its slot is handed through as is.
///
/// # Safety
/// `src` must be readable for `payload.min(slot)` bytes.
pub unsafe fn with_slot_bytes<R>(
    src: *const u8,
    payload: usize,
    slot: usize,
    f: impl FnOnce(*const u8) -> R,
) -> R {
    if payload == slot {
        return f(src);
    }
    if slot <= STACK_SLOT_BYTES {
        let mut scratch = [0u64; STACK_SLOT_BYTES / size_of::<u64>()];
        let dest = scratch.as_mut_ptr() as *mut u8;
        write_slot(dest, src, payload, slot);
        return f(dest);
    }
    let mut scratch = vec![0u8; slot];
    write_slot(scratch.as_mut_ptr(), src, payload, slot);
    f(scratch.as_ptr())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_payload_no_wider_than_its_slot_fits() {
        assert!(fits_slot(1, 8));
        assert!(fits_slot(12, 16));
        assert!(fits_slot(16, 16));
    }

    #[test]
    fn a_value_word_fits_a_slot_no_wider_than_itself() {
        assert!(fits_slot(VALUE_WORD, 1));
        assert!(fits_slot(VALUE_WORD, VALUE_WORD));
        assert!(!fits_slot(VALUE_WORD, 16));
    }

    #[test]
    fn a_payload_wider_than_its_slot_does_not_fit() {
        assert!(!fits_slot(16, 8));
        assert!(!fits_slot(4, 2));
    }

    #[test]
    fn write_slot_zeroes_the_bytes_the_element_does_not_cover() {
        let src = [0xABu8; 4];
        let mut dest = [0xFFu8; 8];
        unsafe { write_slot(dest.as_mut_ptr(), src.as_ptr(), 4, 8) };
        assert_eq!(dest, [0xAB, 0xAB, 0xAB, 0xAB, 0, 0, 0, 0]);
    }

    #[test]
    fn read_slot_hands_back_a_wide_element_whole() {
        let slot = 0x0102_0304_0506_0708_1112_1314_1516_1718u128.to_le_bytes();
        let mut dest = [0u8; 16];
        unsafe { read_slot(dest.as_mut_ptr(), slot.as_ptr(), 16, 16) };
        assert_eq!(dest, slot);
    }

    #[test]
    fn read_slot_hands_back_a_narrow_element_at_its_own_width() {
        let slot = 0xC8usize.to_le_bytes();
        let mut dest = [0xFFu8; 1];
        unsafe { read_slot(dest.as_mut_ptr(), slot.as_ptr(), 1, VALUE_WORD) };
        assert_eq!(dest, [0xC8]);
    }

    #[test]
    fn read_slot_fills_a_value_word_from_a_narrow_slot_with_zeroes() {
        let slot = [0xABu8, 0xCD];
        let mut dest = [0xFFu8; VALUE_WORD];
        unsafe { read_slot(dest.as_mut_ptr(), slot.as_ptr(), VALUE_WORD, 2) };
        assert_eq!(usize::from_le_bytes(dest), 0xCDAB);
    }

    #[test]
    fn read_slot_of_an_absent_element_writes_zero() {
        let mut dest = [0xFFu8; 16];
        unsafe { read_slot(dest.as_mut_ptr(), ptr::null(), 16, 16) };
        assert_eq!(dest, [0u8; 16]);
    }

    #[test]
    fn write_slot_keeps_the_low_bytes_of_a_word_into_a_narrow_slot() {
        let word = 0x1122_3344_5566_7788usize.to_le_bytes();
        let mut dest = [0u8; 2];
        unsafe { write_slot(dest.as_mut_ptr(), word.as_ptr(), VALUE_WORD, 2) };
        assert_eq!(dest, [0x88, 0x77]);
    }

    #[test]
    fn with_slot_bytes_pads_a_narrow_element_on_the_stack_and_the_heap() {
        let src = [7u8; 3];
        let padded = unsafe { with_slot_bytes(src.as_ptr(), 3, 8, |p| *(p as *const [u8; 8])) };
        assert_eq!(padded, [7, 7, 7, 0, 0, 0, 0, 0]);
        let wide = unsafe { with_slot_bytes(src.as_ptr(), 3, 80, |p| *(p.add(79)) + *(p.add(2))) };
        assert_eq!(wide, 7);
    }
}
