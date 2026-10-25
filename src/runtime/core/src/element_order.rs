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

    /// How the element stored at `left` orders against the one at `right`.
    unsafe fn compare(&self, left: *const u8, right: *const u8, elem_size: usize) -> Ordering {
        if self.compare_fn != 0 {
            return self.compare_through_callback(left, right);
        }
        match self.kind {
            BY_UNSIGNED_VALUE => compare_unsigned(left, right, elem_size),
            BY_FLOAT_VALUE => compare_floats(left, right, elem_size),
            _ => compare_signed(left, right, elem_size),
        }
    }

    /// Asks the element type's `compare` about the two values the slots hold.
    unsafe fn compare_through_callback(&self, left: *const u8, right: *const u8) -> Ordering {
        let compare: ElementCompareFn = std::mem::transmute(self.compare_fn);
        let left_value = (left as *const *const u8).read_unaligned();
        let right_value = (right as *const *const u8).read_unaligned();
        compare(left_value, right_value).cmp(&0)
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
/// The positions are sorted with the standard library's stable sort, which
/// makes O(n log n) comparisons and keeps elements that compare equal in the
/// order they were given in; the element bytes are then moved into place once,
/// through a buffer. An element is moved, never dropped or cloned, so whatever
/// a managed element owns stays owned by exactly one slot.
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
    let Some(total) = len.checked_mul(elem_size) else {
        return;
    };
    if order.compare_fn == 0 && sort_scalars(data, len, elem_size, order.kind) {
        return;
    }
    let element = |i: usize| data.add(i * elem_size) as *const u8;
    let mut positions: Vec<usize> = (0..len).collect();
    positions.sort_by(|&a, &b| unsafe { order.compare(element(a), element(b), elem_size) });
    let mut sorted: Vec<u8> = Vec::with_capacity(total);
    for &position in &positions {
        sorted.extend_from_slice(std::slice::from_raw_parts(element(position), elem_size));
    }
    std::ptr::copy_nonoverlapping(sorted.as_ptr(), data, total);
}

/// Sorts elements whose bytes are their value directly as a slice of their
/// type, returning false, having touched nothing, when the width has no such
/// type or the buffer is not aligned for it.
///
/// Every rule here is a total order under which two elements compare equal
/// only when their bits are identical (floats by `total_cmp`), so an unstable
/// sort leaves exactly what the stable index sort above leaves.
///
/// # Safety
///
/// As for [`sort_elements`].
unsafe fn sort_scalars(data: *mut u8, len: usize, elem_size: usize, kind: usize) -> bool {
    match (kind, elem_size) {
        (BY_FLOAT_VALUE, 4) => sort_slice_by(data, len, f32::total_cmp),
        (BY_FLOAT_VALUE, 8) => sort_slice_by(data, len, f64::total_cmp),
        (BY_UNSIGNED_VALUE, 1) => sort_slice_by(data, len, u8::cmp),
        (BY_UNSIGNED_VALUE, 2) => sort_slice_by(data, len, u16::cmp),
        (BY_UNSIGNED_VALUE, 4) => sort_slice_by(data, len, u32::cmp),
        (BY_UNSIGNED_VALUE, 8) => sort_slice_by(data, len, u64::cmp),
        (BY_UNSIGNED_VALUE, 16) => sort_slice_by(data, len, u128::cmp),
        (BY_SIGNED_VALUE, 1) => sort_slice_by(data, len, i8::cmp),
        (BY_SIGNED_VALUE, 2) => sort_slice_by(data, len, i16::cmp),
        (BY_SIGNED_VALUE, 4) => sort_slice_by(data, len, i32::cmp),
        (BY_SIGNED_VALUE, 8) => sort_slice_by(data, len, i64::cmp),
        (BY_SIGNED_VALUE, 16) => sort_slice_by(data, len, i128::cmp),
        _ => false,
    }
}

/// Sorts `len` values of `T` at `data` by `compare`, unstably, or returns
/// false when `data` is not aligned for `T`.
///
/// # Safety
///
/// `data` must point at `len` initialized values of `T` nothing else touches.
unsafe fn sort_slice_by<T>(data: *mut u8, len: usize, compare: fn(&T, &T) -> Ordering) -> bool {
    if !(data as usize).is_multiple_of(std::mem::align_of::<T>()) {
        return false;
    }
    std::slice::from_raw_parts_mut(data as *mut T, len).sort_unstable_by(compare);
    true
}

/// The longest run the merge sort orders by insertion before it starts merging.
const MAX_INSERTION_RUN: usize = 32;

/// Sorts `len` elements of `elem_size` bytes each, in place, by `cmp`.
///
/// A stable, bottom-up merge sort: runs of 16 to 32 elements are ordered by
/// binary insertion, then merged pairwise between the elements and one scratch
/// buffer of the same size — two runs of equal length from both ends at once —
/// and two runs already in order are copied rather than merged. It asks `cmp`
/// at most n·⌈log₂ n⌉ + n times for a comparator that is an order. An element is only
/// moved, never dropped or cloned.
///
/// `cmp(a, b)` answers how the element at `a` orders against the one at `b`.
/// Of two elements, the later is placed first only when `cmp` calls it `Less`
/// than the earlier, which is what keeps equal elements in their order. A
/// comparator that is not a total order still yields a permutation of the
/// elements: the sort never reads outside them and never panics. When the
/// scratch buffer cannot be allocated, the elements are sorted by binary
/// insertion alone, which needs none.
///
/// # Safety
///
/// `data` must point at `len * elem_size` initialized, writable bytes that
/// nothing else reads or writes while the sort runs, `cmp` included.
pub(crate) unsafe fn sort_elements_by(
    data: *mut u8,
    len: usize,
    elem_size: usize,
    mut cmp: impl FnMut(*const u8, *const u8) -> Ordering,
) {
    if data.is_null() || len < 2 || elem_size == 0 {
        return;
    }
    let Some(total) = len.checked_mul(elem_size) else {
        return;
    };
    let elements = Elements {
        base: data,
        size: elem_size,
    };
    let Some(mut scratch) = total.checked_add(elem_size).and_then(scratch_buffer) else {
        insertion_sort_alone(elements, len, &mut cmp);
        return;
    };
    let spare = scratch.as_mut_ptr() as *mut u8;
    let run = insertion_run(len);
    let mut start = 0;
    while start < len {
        let count = run.min(len - start);
        insertion_sort(elements.offset(start), count, spare.add(total), &mut cmp);
        start += count;
    }
    let mut source = elements;
    let mut target = Elements {
        base: spare,
        size: elem_size,
    };
    let mut width = run;
    while width < len {
        merge_pass(source, target, len, width, &mut cmp);
        std::mem::swap(&mut source, &mut target);
        width = width.saturating_mul(2);
    }
    if source.base != data {
        std::ptr::copy_nonoverlapping(source.base, data, total);
    }
}

/// Uninitialized room for `bytes` bytes, aligned for any element a list
/// holds, or `None` when it cannot be allocated.
fn scratch_buffer(bytes: usize) -> Option<Vec<u128>> {
    let mut buffer = Vec::new();
    buffer
        .try_reserve_exact(bytes.div_ceil(std::mem::size_of::<u128>()))
        .ok()?;
    Some(buffer)
}

/// A run of elements of one size, starting at `base`.
#[derive(Clone, Copy)]
struct Elements {
    base: *mut u8,
    size: usize,
}

impl Elements {
    /// The address of element `index`.
    unsafe fn at(self, index: usize) -> *mut u8 {
        self.base.add(index * self.size)
    }

    /// The elements from `index` on.
    unsafe fn offset(self, index: usize) -> Self {
        Self {
            base: self.at(index),
            size: self.size,
        }
    }

    /// Copy `count` elements from `from` here into `to` in `target`.
    unsafe fn copy_to(self, from: usize, target: Elements, to: usize, count: usize) {
        std::ptr::copy_nonoverlapping(self.at(from), target.at(to), count * self.size);
    }

    /// Copy the element at `from` here into `to` in `target`.
    #[inline(always)]
    unsafe fn copy_one_to(self, from: usize, target: Elements, to: usize) {
        copy_element(self.at(from), target.at(to), self.size);
    }
}

/// Copy one element of `size` bytes from `from` to `to`, which do not overlap.
///
/// The sort moves one element at a time, so the common widths are copied at a
/// length known here, as a load and a store rather than a call.
#[inline(always)]
unsafe fn copy_element(from: *const u8, to: *mut u8, size: usize) {
    match size {
        8 => std::ptr::copy_nonoverlapping(from, to, 8),
        16 => std::ptr::copy_nonoverlapping(from, to, 16),
        4 => std::ptr::copy_nonoverlapping(from, to, 4),
        other => std::ptr::copy_nonoverlapping(from, to, other),
    }
}

/// How many elements each run ordered by insertion holds: between 16 and 32
/// for any length of 32 or more, chosen so the number of runs is a power of
/// two or just under one and every merge pass pairs runs of like length.
fn insertion_run(len: usize) -> usize {
    let mut remaining = len;
    let mut carry = 0;
    while remaining >= MAX_INSERTION_RUN {
        carry |= remaining & 1;
        remaining >>= 1;
    }
    remaining + carry
}

/// Orders `count` elements in place by binary insertion, through the
/// element-sized buffer `spare`. Each element is placed after every earlier one
/// `cmp` does not call greater, so equal elements keep their order. The
/// elements it passes move up one at a time: a run is short, and a load and a
/// store each cost less than a call to move them all.
unsafe fn insertion_sort(
    elements: Elements,
    count: usize,
    spare: *mut u8,
    cmp: &mut impl FnMut(*const u8, *const u8) -> Ordering,
) {
    for index in 1..count {
        let element = elements.at(index);
        let low = insertion_point(elements, index, cmp);
        if low < index {
            let size = elements.size;
            copy_element(element, spare, size);
            for passed in (low..index).rev() {
                elements.copy_one_to(passed, elements, passed + 1);
            }
            copy_element(spare, elements.at(low), size);
        }
    }
}

/// Where the element at `index` goes among the ordered elements before it:
/// after every one `cmp` does not call it `Less` than.
unsafe fn insertion_point(
    elements: Elements,
    index: usize,
    cmp: &mut impl FnMut(*const u8, *const u8) -> Ordering,
) -> usize {
    let element = elements.at(index);
    let (mut low, mut high) = (0, index);
    while low < high {
        let middle = low + (high - low) / 2;
        if cmp(element, elements.at(middle)) == Ordering::Less {
            high = middle;
        } else {
            low = middle + 1;
        }
    }
    low
}

/// Orders `len` elements by binary insertion alone, rotating each into place a
/// byte at a time so no buffer is needed: the fallback when the scratch buffer
/// cannot be allocated.
unsafe fn insertion_sort_alone(
    elements: Elements,
    len: usize,
    cmp: &mut impl FnMut(*const u8, *const u8) -> Ordering,
) {
    for index in 1..len {
        let low = insertion_point(elements, index, cmp);
        let moved = (index + 1 - low) * elements.size;
        std::slice::from_raw_parts_mut(elements.at(low), moved).rotate_right(elements.size);
    }
}

/// Merge each pair of adjacent `width`-element runs of `source` into `target`.
unsafe fn merge_pass(
    source: Elements,
    target: Elements,
    len: usize,
    width: usize,
    cmp: &mut impl FnMut(*const u8, *const u8) -> Ordering,
) {
    let mut low = 0;
    while low < len {
        let middle = low.saturating_add(width).min(len);
        let high = middle.saturating_add(width).min(len);
        merge_runs(source, target, (low, middle, high), cmp);
        low = high;
    }
}

/// Merge the ordered runs `low..middle` and `middle..high` of `source` into
/// the same positions of `target`. Two runs already in order are copied whole.
unsafe fn merge_runs(
    source: Elements,
    target: Elements,
    (low, middle, high): (usize, usize, usize),
    cmp: &mut impl FnMut(*const u8, *const u8) -> Ordering,
) {
    if middle == high || cmp(source.at(middle), source.at(middle - 1)) != Ordering::Less {
        source.copy_to(low, target, low, high - low);
        return;
    }
    let runs = (low, middle, high);
    if middle - low == high - middle && merge_from_both_ends(source, target, runs, cmp) {
        return;
    }
    merge_from_the_front(source, target, runs, cmp);
}

/// Merge the ordered runs `low..middle` and `middle..high` of `source` into
/// `target`, smallest first.
unsafe fn merge_from_the_front(
    source: Elements,
    target: Elements,
    (low, middle, high): (usize, usize, usize),
    cmp: &mut impl FnMut(*const u8, *const u8) -> Ordering,
) {
    let (mut left, mut right, mut out) = (low, middle, low);
    while left < middle && right < high {
        // Chosen without a branch on the answer, which on unsorted input is
        // as likely one way as the other and would be mispredicted half the
        // time.
        let takes_right = cmp(source.at(right), source.at(left)) == Ordering::Less;
        let take = if takes_right { right } else { left };
        source.copy_one_to(take, target, out);
        right += usize::from(takes_right);
        left += usize::from(!takes_right);
        out += 1;
    }
    source.copy_to(left, target, out, middle - left);
    source.copy_to(right, target, out + middle - left, high - right);
}

/// Merge the ordered runs `low..middle` and `middle..high` of `source`, of
/// equal length, into `target` from both ends at once: each step places the
/// smallest element left at the front and the greatest at the back, two
/// comparisons that do not wait on each other.
///
/// Each end takes half the elements, so with runs of equal length neither
/// runs out before the ends meet, and no index leaves its run. Returns whether
/// the ends met exactly, which they do for every comparator that is an order;
/// when they did not, `target` holds no permutation and the caller merges
/// again from the front.
unsafe fn merge_from_both_ends(
    source: Elements,
    target: Elements,
    (low, middle, high): (usize, usize, usize),
    cmp: &mut impl FnMut(*const u8, *const u8) -> Ordering,
) -> bool {
    let (mut left, mut right, mut out) = (low, middle, low);
    let (mut left_back, mut right_back, mut out_back) = (middle - 1, high - 1, high - 1);
    for _ in 0..(high - low) / 2 {
        let takes_right = cmp(source.at(right), source.at(left)) == Ordering::Less;
        source.copy_one_to(if takes_right { right } else { left }, target, out);
        right += usize::from(takes_right);
        left += usize::from(!takes_right);
        out += 1;

        let takes_left = cmp(source.at(right_back), source.at(left_back)) == Ordering::Less;
        let back = if takes_left { left_back } else { right_back };
        source.copy_one_to(back, target, out_back);
        left_back = left_back.wrapping_sub(usize::from(takes_left));
        right_back = right_back.wrapping_sub(usize::from(!takes_left));
        out_back = out_back.wrapping_sub(1);
    }
    left == left_back.wrapping_add(1) && right == right_back.wrapping_add(1)
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

    /// The order the generic path gives: a stable sort of the elements by
    /// `order.compare`, the reference the scalar fast paths must match.
    fn generically_sorted(bytes: &[u8], elem_size: usize, order: ElementOrder) -> Vec<u8> {
        let mut chunks: Vec<&[u8]> = bytes.chunks(elem_size).collect();
        chunks.sort_by(|a, b| unsafe { order.compare(a.as_ptr(), b.as_ptr(), elem_size) });
        chunks.concat()
    }

    /// Sorts `values` through `sort_elements` and checks the bytes equal the
    /// generic order's, for an aligned buffer and for one shifted off its
    /// alignment, which must take the generic path and agree as well.
    fn assert_sorts_like_the_generic_path<T: Copy>(values: &[T], order: ElementOrder) {
        let size = std::mem::size_of::<T>();
        let bytes: Vec<u8> = values
            .iter()
            .flat_map(|v| {
                unsafe { std::slice::from_raw_parts(v as *const T as *const u8, size) }.to_vec()
            })
            .collect();
        let expected = generically_sorted(&bytes, size, order);
        let mut aligned: Vec<T> = values.to_vec();
        let mut shifted = vec![0u8; bytes.len() + 1];
        shifted[1..].copy_from_slice(&bytes);
        unsafe {
            sort_elements(aligned.as_mut_ptr() as *mut u8, values.len(), size, order);
            sort_elements(shifted.as_mut_ptr().add(1), values.len(), size, order);
            let sorted = std::slice::from_raw_parts(aligned.as_ptr() as *const u8, bytes.len());
            assert_eq!(sorted, expected.as_slice(), "aligned, {size}-byte elements");
        }
        assert_eq!(
            &shifted[1..],
            expected.as_slice(),
            "unaligned, {size}-byte elements"
        );
    }

    #[test]
    fn scalar_elements_sort_exactly_as_the_generic_order_does() {
        let mut state = 7u64;
        let mut next = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            state
        };
        let wide: Vec<u64> = (0..500).map(|_| next()).collect();
        let signed = ordered_by(BY_SIGNED_VALUE);
        let unsigned = ordered_by(BY_UNSIGNED_VALUE);
        let floats = ordered_by(BY_FLOAT_VALUE);
        assert_sorts_like_the_generic_path(
            &wide.iter().map(|&v| v as i8).collect::<Vec<_>>(),
            signed,
        );
        assert_sorts_like_the_generic_path(
            &wide.iter().map(|&v| v as i16).collect::<Vec<_>>(),
            signed,
        );
        assert_sorts_like_the_generic_path(
            &wide.iter().map(|&v| v as i32 % 50).collect::<Vec<_>>(),
            signed,
        );
        assert_sorts_like_the_generic_path(
            &wide.iter().map(|&v| v as i64).collect::<Vec<_>>(),
            signed,
        );
        assert_sorts_like_the_generic_path(
            &wide
                .iter()
                .map(|&v| ((v as i128) << 64) | v as i128)
                .collect::<Vec<_>>(),
            signed,
        );
        assert_sorts_like_the_generic_path(
            &wide.iter().map(|&v| v as u8).collect::<Vec<_>>(),
            unsigned,
        );
        assert_sorts_like_the_generic_path(
            &wide.iter().map(|&v| v as u32).collect::<Vec<_>>(),
            unsigned,
        );
        assert_sorts_like_the_generic_path(&wide, unsigned);
        assert_sorts_like_the_generic_path(
            &wide.iter().map(|&v| v as u128 * 3).collect::<Vec<_>>(),
            unsigned,
        );
        let mut doubles: Vec<f64> = wide.iter().map(|&v| (v % 1000) as f64 - 500.0).collect();
        doubles.extend([
            0.0,
            -0.0,
            f64::NAN,
            -f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            0.0,
            -0.0,
        ]);
        assert_sorts_like_the_generic_path(&doubles, floats);
        let singles: Vec<f32> = doubles.iter().map(|&v| v as f32).collect();
        assert_sorts_like_the_generic_path(&singles, floats);
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

    static COMPARISONS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// Orders two `i64` values and counts how often it is asked.
    unsafe extern "C" fn counting_compare(a: *const u8, b: *const u8) -> isize {
        COMPARISONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (a, b) = ((a as *const i64).read(), (b as *const i64).read());
        a.cmp(&b) as isize
    }

    #[test]
    fn sorting_takes_n_log_n_comparisons_not_n_squared() {
        let n = 1000usize;
        let values: Vec<i64> = (0..n as i64).map(|i| (i * 7919) % n as i64).collect();
        let mut slots: Vec<*const i64> = values.iter().map(|v| v as *const i64).collect();
        COMPARISONS.store(0, std::sync::atomic::Ordering::Relaxed);
        unsafe {
            sort_elements(
                slots.as_mut_ptr() as *mut u8,
                n,
                std::mem::size_of::<*const i64>(),
                ordered_through(counting_compare),
            );
        }
        let sorted: Vec<i64> = slots.iter().map(|&p| unsafe { *p }).collect();
        assert_eq!(sorted, (0..n as i64).collect::<Vec<_>>());
        let calls = COMPARISONS.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            calls <= 2 * n * 10,
            "{calls} comparisons to sort {n} elements"
        );
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

    /// Sorts `pairs` by their first field through `sort_elements_by`, counting
    /// the comparisons it asks for.
    fn sort_pairs_by_key(pairs: &mut [(i64, i64)]) -> usize {
        let mut asked = 0usize;
        unsafe {
            sort_elements_by(
                pairs.as_mut_ptr() as *mut u8,
                pairs.len(),
                std::mem::size_of::<(i64, i64)>(),
                |left, right| {
                    asked += 1;
                    let left = (left as *const (i64, i64)).read();
                    let right = (right as *const (i64, i64)).read();
                    left.0.cmp(&right.0)
                },
            );
        }
        asked
    }

    /// A deterministic scramble of `0..n`, keyed into `keys` buckets, each
    /// pair carrying its original position as its second field.
    fn scrambled_pairs(n: usize, keys: i64) -> Vec<(i64, i64)> {
        (0..n as i64)
            .map(|i| (((i * 7919) + 13) % (n as i64).max(1) % keys, i))
            .collect()
    }

    #[test]
    fn sorting_by_a_comparator_keeps_equal_elements_in_their_order() {
        for n in [3usize, 17, 33, 100, 1000, 4097] {
            let mut pairs = scrambled_pairs(n, 7);
            sort_pairs_by_key(&mut pairs);
            for window in pairs.windows(2) {
                assert!(window[0].0 <= window[1].0, "{n}: {window:?} out of order");
                if window[0].0 == window[1].0 {
                    assert!(window[0].1 < window[1].1, "{n}: {window:?} reordered");
                }
            }
        }
    }

    #[test]
    fn sorting_by_a_comparator_handles_short_and_odd_lengths() {
        for n in [0usize, 1, 2, 5, 31, 33, 63, 65, 999] {
            let mut pairs: Vec<(i64, i64)> = (0..n as i64).rev().map(|i| (i, i)).collect();
            sort_pairs_by_key(&mut pairs);
            let expected: Vec<(i64, i64)> = (0..n as i64).map(|i| (i, i)).collect();
            assert_eq!(pairs, expected, "length {n}");
        }
        unsafe {
            sort_elements_by(std::ptr::null_mut(), 4, 8, |_, _| Ordering::Less);
        }
    }

    #[test]
    fn sorting_by_a_comparator_asks_at_most_n_log_n_plus_n_times() {
        for n in [2usize, 10, 100, 1000, 1 << 12, 5000] {
            let bound = n * (n as f64).log2().ceil() as usize + n;
            let mut scrambled = scrambled_pairs(n, n as i64);
            let mut descending: Vec<(i64, i64)> = (0..n as i64).rev().map(|i| (i, i)).collect();
            let mut ascending: Vec<(i64, i64)> = (0..n as i64).map(|i| (i, i)).collect();
            for pairs in [&mut scrambled, &mut descending, &mut ascending] {
                let asked = sort_pairs_by_key(pairs);
                assert!(asked <= bound, "{asked} comparisons for {n} elements");
            }
        }
    }

    #[test]
    fn a_comparator_that_is_not_an_order_still_yields_a_permutation() {
        for n in [2usize, 7, 40, 1000] {
            let mut pairs: Vec<(i64, i64)> = (0..n as i64).map(|i| (i, i)).collect();
            let mut flip = 0u32;
            unsafe {
                sort_elements_by(
                    pairs.as_mut_ptr() as *mut u8,
                    n,
                    std::mem::size_of::<(i64, i64)>(),
                    |_, _| {
                        flip = flip.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                        match flip % 3 {
                            0 => Ordering::Less,
                            1 => Ordering::Equal,
                            _ => Ordering::Greater,
                        }
                    },
                );
            }
            let mut seen: Vec<i64> = pairs.iter().map(|pair| pair.1).collect();
            seen.sort_unstable();
            assert_eq!(seen, (0..n as i64).collect::<Vec<_>>(), "length {n}");
        }
    }

    #[test]
    fn sorting_without_a_scratch_buffer_is_stable_too() {
        let mut pairs = scrambled_pairs(300, 5);
        let elements = Elements {
            base: pairs.as_mut_ptr() as *mut u8,
            size: std::mem::size_of::<(i64, i64)>(),
        };
        unsafe {
            insertion_sort_alone(elements, pairs.len(), &mut |left, right| {
                let left = (left as *const (i64, i64)).read();
                let right = (right as *const (i64, i64)).read();
                left.0.cmp(&right.0)
            });
        }
        assert!(pairs
            .windows(2)
            .all(|w| (w[0].0, w[0].1) < (w[1].0, w[1].1)));
    }
}
