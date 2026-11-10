// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The memory layout compiled code and the runtime both rely on: where a
//! collection or string header keeps each word, where a value's count lives
//! and which counts mark a value immortal, what the heap guard's tracking
//! state means, and how a container's element rules are encoded.
//!
//! The compiler does not link the runtime and the runtime does not link the
//! compiler, so this file is compiled into both: the compiler as
//! `crate::abi::layout`, the runtime through a `#[path]` module. It depends on
//! nothing but `core`. The runtime asserts at compile time that each of its
//! own structs puts every word named here where this file says; code
//! generation reads every offset from here.
//!
//! A header word is named by its index, not its byte offset: compiled code
//! multiplies by the target's pointer size. Each header below holds only
//! pointer-sized fields, which a compile-time assertion checks, so an index
//! does not depend on the host the compiler runs on.

use core::mem::{offset_of, size_of};

/// One pointer-sized header field.
type Word = usize;

/// The words a list's header begins with, in order.
#[repr(C)]
pub struct ListHeader {
    pub data: Word,
    pub len: Word,
    pub capacity: Word,
    pub elem_size: Word,
}

/// The words an array's header begins with, in order.
#[repr(C)]
pub struct ArrayHeader {
    pub data: Word,
    pub elem_count: Word,
    pub elem_size: Word,
}

/// The words a map's header begins with, in order.
#[repr(C)]
pub struct MapHeader {
    pub states: Word,
    pub keys: Word,
    pub values: Word,
    pub len: Word,
}

/// The words a set's header begins with, in order.
#[repr(C)]
pub struct SetHeader {
    pub data: Word,
    pub len: Word,
}

/// The words a string's header begins with, in order. The word after them
/// holds the ASCII flag in its first byte and the storage kind in the next.
#[repr(C)]
pub struct StringHeader {
    pub data: Word,
    pub len: Word,
    pub capacity: Word,
}

/// The words a closure's payload begins with, in order: the function a call
/// of the closure enters, and the drop thunk of its environment, or null when
/// releasing the environment releases nothing. The captures follow them.
///
/// The drop word is a drop thunk under the contract every drop thunk keeps:
/// called once the closure's count reaches zero, it releases the captures and
/// frees the block.
#[repr(C)]
pub struct ClosureHeader {
    pub function: Word,
    pub drop: Word,
}

const _: () = assert!(size_of::<ListHeader>() == 4 * size_of::<Word>());
const _: () = assert!(size_of::<ArrayHeader>() == 3 * size_of::<Word>());
const _: () = assert!(size_of::<MapHeader>() == 4 * size_of::<Word>());
const _: () = assert!(size_of::<SetHeader>() == 2 * size_of::<Word>());
const _: () = assert!(size_of::<StringHeader>() == 3 * size_of::<Word>());
const _: () = assert!(size_of::<ClosureHeader>() == 2 * size_of::<Word>());

/// The word of a list's header holding the address of its first element.
pub const LIST_DATA: usize = word_index(offset_of!(ListHeader, data));
/// The word of a list's header holding its element count.
pub const LIST_LEN: usize = word_index(offset_of!(ListHeader, len));
/// The word of a list's header holding how many elements fit before it grows.
pub const LIST_CAPACITY: usize = word_index(offset_of!(ListHeader, capacity));
/// The word of a list's header holding the byte width of one element slot.
pub const LIST_SLOT_SIZE: usize = word_index(offset_of!(ListHeader, elem_size));

/// The word of an array's header holding the address of its first element.
pub const ARRAY_DATA: usize = word_index(offset_of!(ArrayHeader, data));
/// The word of an array's header holding its element count.
pub const ARRAY_LEN: usize = word_index(offset_of!(ArrayHeader, elem_count));
/// The word of an array's header holding the byte width of one element slot.
pub const ARRAY_SLOT_SIZE: usize = word_index(offset_of!(ArrayHeader, elem_size));

/// The word of an array's or a list's header holding the address of its first
/// element. The two headers agree on it, so one element read serves either.
pub const ELEMENTS_DATA: usize = LIST_DATA;
/// The word of an array's or a list's header holding its element count.
pub const ELEMENTS_LEN: usize = LIST_LEN;
const _: () = assert!(LIST_DATA == ARRAY_DATA && LIST_LEN == ARRAY_LEN);

/// The word of a closure's payload holding the function a call enters.
pub const CLOSURE_FUNCTION: usize = word_index(offset_of!(ClosureHeader, function));
/// The word of a closure's payload holding its environment's drop thunk.
pub const CLOSURE_DROP: usize = word_index(offset_of!(ClosureHeader, drop));
/// The word of a closure's payload its first capture starts at.
pub const CLOSURE_CAPTURES: usize = size_of::<ClosureHeader>() / size_of::<Word>();

/// The word of a map's header holding its entry count.
pub const MAP_LEN: usize = word_index(offset_of!(MapHeader, len));

/// The word of a set's header holding its element count.
pub const SET_LEN: usize = word_index(offset_of!(SetHeader, len));

/// The word of a string's header holding the address of its bytes.
pub const STRING_DATA: usize = word_index(offset_of!(StringHeader, data));
/// The word of a string's header holding its byte length.
pub const STRING_LEN: usize = word_index(offset_of!(StringHeader, len));
/// The word of a string's header holding how many bytes fit before it grows.
pub const STRING_CAPACITY: usize = word_index(offset_of!(StringHeader, capacity));
/// The word of a string's header whose first byte is its ASCII flag.
pub const STRING_ASCII: usize = size_of::<StringHeader>() / size_of::<Word>();
/// The byte, within the [`STRING_ASCII`] word, holding the storage kind.
pub const STRING_STORAGE_BYTE: usize = 1;

/// The storage kind of bytes a string does not own, such as a literal's. They
/// are never freed.
pub const STORAGE_UNOWNED: u8 = 0;
/// The storage kind of a buffer the string owns and frees.
pub const STORAGE_HEAP: u8 = 1;
/// The storage kind of bytes held in the string header's own block.
pub const STORAGE_INLINE: u8 = 2;

/// How many words a counted value's count takes, just before its payload.
pub const COUNT_WORDS: usize = 1;

/// Whether a value whose count word reads `count` is immortal: no retain or
/// release changes it and it is never freed. Every count with the word's high
/// bit set is. A string literal is emitted with only that bit set, and a value
/// whose drop hook is running is given that count, so the hook's own retains
/// and releases leave it alone.
pub const fn is_immortal(count: usize) -> bool {
    (count as isize) < 0
}

/// Compiled code has not yet learned whether the runtime wants to observe its
/// allocations; its first allocation calls the runtime, which settles it.
pub const TRACKING_UNSET: u8 = 0;
/// The runtime does not want to observe allocations; compiled code skips the
/// call that would report one.
pub const TRACKING_OFF: u8 = 1;
/// The runtime wants every allocation and release reported.
pub const TRACKING_ON: u8 = 2;

/// The element rule matching an element by its bytes; every container starts
/// at it.
pub const BY_BYTES: usize = 0;
/// The element rule matching an element pointing at a string by its content.
pub const BY_STRING_CONTENT: usize = 1;
/// The element rule matching a half-precision float by the number it holds.
pub const BY_F16_VALUE: usize = 2;
/// The element rule matching a single-precision float by the number it holds.
pub const BY_F32_VALUE: usize = 3;
/// The element rule matching a double-precision float by the number it holds.
pub const BY_F64_VALUE: usize = 4;

/// Width of the field of an element kind word holding the rule.
pub const RULE_BITS: u32 = 8;
/// Width of the field holding how many boxed optionals wrap the element.
pub const DEPTH_BITS: u32 = 8;
/// The bit, just above the depth, saying the innermost optional is a nullable
/// pointer rather than a box.
pub const NULLABLE_BIT: u32 = RULE_BITS + DEPTH_BITS;
/// The bit, just above the nullable bit, saying a callback is handed the
/// address the element resolves to rather than the word stored there.
pub const SLOT_ADDRESS_BIT: u32 = NULLABLE_BIT + 1;
/// Where the boxed value's size starts, above the slot-address bit.
pub const SIZE_SHIFT: u32 = SLOT_ADDRESS_BIT + 1;
/// How many boxed optionals the depth field can say an element is wrapped in.
pub const MAX_OPTIONAL_DEPTH: usize = (1 << DEPTH_BITS) - 1;

/// The element kind word registering `rule` for a value reached by opening
/// `boxed_depth` boxed optionals, the last box holding `value_size` bytes,
/// and then, when `innermost_nullable` is set, one optional more whose `None`
/// is a null pointer. With no optional at all the word is `rule` itself.
pub const fn through_optionals(
    rule: usize,
    boxed_depth: usize,
    value_size: usize,
    innermost_nullable: bool,
) -> usize {
    rule | (boxed_depth << RULE_BITS)
        | ((innermost_nullable as usize) << NULLABLE_BIT)
        | (value_size << SIZE_SHIFT)
}

/// `kind`, with callbacks handed the address the element resolves to: the
/// kind word of an element held inline as its bytes.
pub const fn by_slot_address(kind: usize) -> usize {
    kind | (1 << SLOT_ADDRESS_BIT)
}

/// The order kind sorting a list's or array's elements as signed integers,
/// which every list and array starts at.
pub const BY_SIGNED_VALUE: usize = 0;
/// The order kind sorting elements as unsigned integers.
pub const BY_UNSIGNED_VALUE: usize = 1;
/// The order kind sorting elements as floats.
pub const BY_FLOAT_VALUE: usize = 2;

/// The index of the word at byte `offset` of a header of words.
const fn word_index(offset: usize) -> usize {
    offset / size_of::<Word>()
}
