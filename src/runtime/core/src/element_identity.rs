// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Deciding whether two elements of a type-erased container are the same.
//!
//! A set asks it of its elements and a map of its keys, and both must answer
//! the way `==` does for the element type, or a membership test disagrees with
//! the comparison the program would write by hand. The runtime knows an element
//! only by its size, so the compiler tells it which of four rules applies:
//!
//! - an element whose bytes *are* its value — an integer, a boolean — is the
//!   same element when its bytes are;
//! - a float is the same element when it is the same number: `-0.0` and `0.0`
//!   are one element, and so is every NaN, so a NaN stored can be found again;
//! - an element whose bytes point at a string is the same element when the two
//!   strings hold the same content, wherever each was allocated;
//! - an element whose type defines its own equality is compared through a
//!   callback the compiler generates from that type's `equals`, and hashed
//!   through one it generates from the same type's `hash`.
//!
//! An optional element is laid out one of two ways, layer by layer, and the
//! compiler says which in the kind word:
//!
//! - a **boxed** layer is the address of its `Some` box, null for `None`. Two
//!   separately built `Some(2)`s hold that same 2 at two addresses, so the
//!   rules above never apply to a box's address: the compiler says how many
//!   boxed layers wrap the element and how wide the value in the innermost box
//!   is, and the rules apply to that value once every box has been opened;
//! - a **nullable pointer** layer, possible only innermost and only around a
//!   managed pointer, is that pointer itself, null for `None`. It is not
//!   opened: once the boxes are, its word is tested for null and otherwise read
//!   by the rule exactly as a bare pointer element would be.
//!
//! A `None` is the absence of a value, equal only to a `None` found at the same
//! layer: `None` is not `Some(None)`, whichever way each layer is laid out.
//!
//! An element held inline as its bytes — a struct or tuple of scalars — is no
//! word at all, so a callback cannot be handed "the value the slot holds": it
//! is handed the address the element resolves to instead, which is where the
//! compiled `equals` and `hash` read such a value from.
//!
//! The kind word packs all of it: bits 0..8 hold the rule, bits 8..16 how many
//! boxed layers wrap the element, bit 16 whether one nullable pointer layer
//! sits inside them, bit 17 whether callbacks are handed the resolved address
//! rather than the word there, and the bits from 18 up the width of the value
//! the innermost box holds. Resolving an element opens each boxed layer in turn,
//! stopping at a null box address as a `None` at that depth; then, when bit 16
//! is set, tests the word it reached for null, a `None` one layer below the
//! boxes; and otherwise hands the value it reached to the rule. With no boxed
//! depth the value is read at the slot's own size, a nullable pointer layer
//! included, since its word is the slot itself.
//!
//! Hashing follows the same rule, since two elements that compare equal have
//! to land in the same probe chain.

use crate::string::MiriString;

/// An equality over two element values, as the compiler generates it.
///
/// Each argument is the value the element slot holds — for a managed element
/// that is the pointer to the value, matching what the drop and clone callbacks
/// are handed — or, for an element held inline as its bytes, the address of
/// those bytes. Returns nonzero when the two are the same element.
pub type ElementEqualsFn = unsafe extern "C" fn(*const u8, *const u8) -> u8;

/// A hash over one element value, as the compiler generates it, consistent
/// with the [`ElementEqualsFn`] registered beside it: two values that one
/// calls equal, this hashes alike. The argument is the value the element slot
/// holds, as the equality is handed it.
pub type ElementHashFn = unsafe extern "C" fn(*const u8) -> i64;

/// The element's bytes are its value.
pub const BY_BYTES: usize = 0;

/// The element's bytes point at a `MiriString`, matched by its content.
pub const BY_STRING_CONTENT: usize = 1;

/// The element is an IEEE half-precision float, matched by the number it
/// holds. A float is read at its own width from the start of its slot, which
/// may be wider.
pub const BY_F16_VALUE: usize = 2;

/// The element is an IEEE single-precision float, matched by its number.
pub const BY_F32_VALUE: usize = 3;

/// The element is an IEEE double-precision float, matched by its number.
pub const BY_F64_VALUE: usize = 4;

/// The width in bytes of the float `rule` matches by number, or `None` when
/// `rule` matches something else.
fn float_width(rule: usize) -> Option<usize> {
    match rule {
        BY_F16_VALUE => Some(2),
        BY_F32_VALUE => Some(4),
        BY_F64_VALUE => Some(8),
        _ => None,
    }
}

/// Width of the field holding the rule that settles a resolved value.
const RULE_BITS: u32 = 8;

/// Width of the field holding how many boxed optionals wrap the element.
const DEPTH_BITS: u32 = 8;

/// The bit, just above the depth, saying the innermost optional is a nullable
/// pointer rather than a box.
const NULLABLE_BIT: u32 = RULE_BITS + DEPTH_BITS;

/// The bit, just above the nullable bit, saying a callback is handed the
/// address the element resolves to rather than the word stored there.
const SLOT_ADDRESS_BIT: u32 = NULLABLE_BIT + 1;

/// Where the boxed value's size starts, above the slot-address bit.
const SIZE_SHIFT: u32 = SLOT_ADDRESS_BIT + 1;

/// How many optionals one element may be wrapped in and still be matched by
/// content. Nesting deeper than this keeps the byte rule: the compiler declines
/// to encode a depth that does not fit, rather than truncating one into a rule
/// that would open the wrong number of boxes.
pub const MAX_OPTIONAL_DEPTH: usize = (1 << DEPTH_BITS) - 1;

/// The kind word registering `rule` for a value reached by opening
/// `boxed_depth` boxed optionals, each box holding `value_size` bytes, and then,
/// when `innermost_nullable` is set, one optional more whose `None` is a null
/// pointer and whose `Some` is the pointer itself.
///
/// With no optional at all — `boxed_depth` zero and no nullable layer — the word
/// is `rule` exactly as the bare constants spell it, so a container whose
/// elements are not optional encodes as it always did.
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

/// What an element slot holds once every optional wrapping it is opened.
enum Resolved {
    /// The address the settling rule reads the value at.
    Value(*const u8),
    /// A `None` was reached, after opening this many boxes.
    Missing(usize),
}

/// How a container decides that two of its elements are the same element.
///
/// Every container starts [`BY_BYTES`] with no callback. The rule must be
/// settled while the container is still empty: an element already stored was
/// placed by the hash of the rule in force when it arrived.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct ElementIdentity {
    /// The rule, the boxed depth, the nullable bit and the boxed value's size,
    /// packed by
    /// [`through_optionals`]. The rule is consulted when `equals_fn` is zero.
    kind: usize,
    /// Address of an [`ElementEqualsFn`], or zero when the element type defines
    /// no equality of its own.
    equals_fn: usize,
    /// Address of the [`ElementHashFn`] consistent with `equals_fn`, or zero
    /// when there is none.
    hash_fn: usize,
}

impl ElementIdentity {
    /// The identity every new container starts with: compare by bytes.
    pub(crate) const BYTES: Self = Self {
        kind: BY_BYTES,
        equals_fn: 0,
        hash_fn: 0,
    };

    /// Selects the rule, as packed by [`through_optionals`].
    pub(crate) fn set_kind(&mut self, kind: usize) {
        self.kind = kind;
    }

    /// Routes equality through the element type's own `equals`.
    pub(crate) fn set_equals_fn(&mut self, equals_fn: usize) {
        self.equals_fn = equals_fn;
    }

    /// Routes hashing through the element type's own `hash`, the one
    /// consistent with the `equals` it is matched through.
    pub(crate) fn set_hash_fn(&mut self, hash_fn: usize) {
        self.hash_fn = hash_fn;
    }

    /// The rule settling two resolved values.
    fn rule(&self) -> usize {
        self.kind & ((1 << RULE_BITS) - 1)
    }

    /// How many boxed optionals wrap the element; zero when none does.
    fn optional_depth(&self) -> usize {
        (self.kind >> RULE_BITS) & MAX_OPTIONAL_DEPTH
    }

    /// True when the innermost optional is a nullable pointer, tested for null
    /// in its slot rather than opened.
    fn innermost_nullable(&self) -> bool {
        (self.kind >> NULLABLE_BIT) & 1 == 1
    }

    /// True when a callback is handed the address the element resolves to,
    /// rather than the word stored there.
    fn hands_over_address(&self) -> bool {
        (self.kind >> SLOT_ADDRESS_BIT) & 1 == 1
    }

    /// The argument a callback is handed for the value resolved at `at`.
    unsafe fn callback_argument(&self, at: *const u8) -> *const u8 {
        if self.hands_over_address() {
            at
        } else {
            *(at as *const *const u8)
        }
    }

    /// The number of bytes the settling rule reads, given the element slot's
    /// own `slot_size`. An optional's boxed value has a width of its own — an
    /// `int?` box holds eight bytes where an `i32?` box holds four, with the
    /// rest of the box never written — so only a value no box holds, the
    /// element itself, is read at the size of its slot.
    fn value_size(&self, slot_size: usize) -> usize {
        if self.optional_depth() == 0 {
            return slot_size;
        }
        self.kind >> SIZE_SHIFT
    }

    /// Opens every optional wrapping the element stored at `elem`.
    ///
    /// Each box holds its value at offset zero, so opening one is reading the
    /// pointer and continuing at what it points to; a null pointer is a `None`
    /// and ends the walk at the depth it was found. A nullable innermost layer
    /// is not opened: its word *is* the payload pointer, so a null word is a
    /// `None` one layer below the boxes, and any other leaves the value at the
    /// slot holding that word — where the rule reads a bare pointer element.
    ///
    /// # Safety
    ///
    /// `elem` must point at an element of this rule, whose optionals are live.
    unsafe fn resolve(&self, elem: *const u8) -> Resolved {
        let mut at = elem;
        for opened in 0..self.optional_depth() {
            let inner = *(at as *const *const u8);
            if inner.is_null() {
                return Resolved::Missing(opened);
            }
            at = inner;
        }
        if self.innermost_nullable() && (*(at as *const *const u8)).is_null() {
            return Resolved::Missing(self.optional_depth());
        }
        Resolved::Value(at)
    }

    /// Hashes the element stored at `elem`, whose slot spans `slot_size` bytes.
    ///
    /// An element compared through a callback is hashed through the callback
    /// registered beside it; without one it hashes to one constant, since any
    /// hash taken from the bytes would separate values the callback calls
    /// equal. The compiler registers both, so the constant is only a fallback.
    ///
    /// # Safety
    ///
    /// `elem` must point at `slot_size` readable bytes holding an element of
    /// this rule.
    pub(crate) unsafe fn hash(&self, elem: *const u8, slot_size: usize) -> u64 {
        match self.resolve(elem) {
            Resolved::Value(at) => self.hash_value(at, self.value_size(slot_size)),
            // A `None` has no bytes to hash, and one found at another depth is
            // a different value — `Some(None)` is not `None` — so the depth is
            // what separates them.
            Resolved::Missing(opened) => {
                let depth = opened.to_ne_bytes();
                crate::hash::fnv1a(depth.as_ptr(), depth.len())
            }
        }
    }

    /// Hashes the value at `at`, which the settling rule reads over `size` bytes.
    unsafe fn hash_value(&self, at: *const u8, size: usize) -> u64 {
        if self.equals_fn != 0 {
            if self.hash_fn == 0 {
                return 0;
            }
            let hash: ElementHashFn = std::mem::transmute(self.hash_fn);
            let value = hash(self.callback_argument(at)).to_ne_bytes();
            return crate::hash::fnv1a(value.as_ptr(), value.len());
        }
        if self.rule() == BY_STRING_CONTENT {
            let (data, len) = string_content(at);
            return crate::hash::fnv1a(data, len);
        }
        if let Some(width) = float_width(self.rule()) {
            let bits = float_value_bits(at, width).to_ne_bytes();
            return crate::hash::fnv1a(bits.as_ptr(), bits.len());
        }
        crate::hash::fnv1a(at, size)
    }

    /// True when the elements stored at `a` and `b`, each occupying a slot of
    /// `slot_size` bytes, are the same element.
    ///
    /// # Safety
    ///
    /// `a` and `b` must each point at `slot_size` readable bytes holding an
    /// element of this rule.
    pub(crate) unsafe fn same(&self, a: *const u8, b: *const u8, slot_size: usize) -> bool {
        match (self.resolve(a), self.resolve(b)) {
            (Resolved::Value(left), Resolved::Value(right)) => {
                self.same_value(left, right, self.value_size(slot_size))
            }
            (Resolved::Missing(left), Resolved::Missing(right)) => left == right,
            (Resolved::Value(_), Resolved::Missing(_))
            | (Resolved::Missing(_), Resolved::Value(_)) => false,
        }
    }

    /// True when the values at `a` and `b`, read over `size` bytes, are the
    /// same value under the settling rule.
    unsafe fn same_value(&self, a: *const u8, b: *const u8, size: usize) -> bool {
        if self.equals_fn != 0 {
            return self.same_through_callback(a, b);
        }
        if self.rule() == BY_STRING_CONTENT {
            let (a_data, a_len) = string_content(a);
            let (b_data, b_len) = string_content(b);
            return a_len == b_len && bytes_equal(a_data, b_data, a_len);
        }
        if let Some(width) = float_width(self.rule()) {
            return float_value_bits(a, width) == float_value_bits(b, width);
        }
        bytes_equal(a, b, size)
    }

    /// Asks the element type's `equals` about the two values the slots hold,
    /// or about the two inline values at `a` and `b`.
    ///
    /// The generated callback treats a null value as equal only to another null,
    /// so the runtime never hands the user's method a missing receiver.
    unsafe fn same_through_callback(&self, a: *const u8, b: *const u8) -> bool {
        let equals: ElementEqualsFn = std::mem::transmute(self.equals_fn);
        equals(self.callback_argument(a), self.callback_argument(b)) != 0
    }
}

/// The bytes of the string an element slot points at; a null string and an
/// empty one both read as no bytes.
unsafe fn string_content(slot: *const u8) -> (*const u8, usize) {
    let string = *(slot as *const *const MiriString);
    if string.is_null() || (*string).data.is_null() {
        return (std::ptr::null(), 0);
    }
    ((*string).data, (*string).len)
}

/// The bits of the float `size` bytes wide at `at`, with every representation of
/// one number made the same: a negative zero reads as the positive one, and
/// every NaN as the one quiet NaN of its width. Two floats a set or map treats
/// as one element have equal results, which is what both hashing and matching
/// compare.
pub(crate) unsafe fn float_value_bits(at: *const u8, size: usize) -> u64 {
    match size {
        2 => normalized_float_bits(
            u64::from((at as *const u16).read_unaligned()),
            15,
            0x7c00,
            0x7e00,
        ),
        4 => normalized_float_bits(
            u64::from((at as *const u32).read_unaligned()),
            31,
            0x7f80_0000,
            0x7fc0_0000,
        ),
        _ => normalized_float_bits(
            (at as *const u64).read_unaligned(),
            63,
            0x7ff0_0000_0000_0000,
            0x7ff8_0000_0000_0000,
        ),
    }
}

/// `bits` of a float whose sign is bit `sign_bit`, whose infinity has the
/// magnitude bits `infinity`, and whose canonical quiet NaN is `quiet_nan`,
/// with a negative zero read as zero and any NaN as `quiet_nan`.
fn normalized_float_bits(bits: u64, sign_bit: u32, infinity: u64, quiet_nan: u64) -> u64 {
    let magnitude = bits & ((1u64 << sign_bit) - 1);
    if magnitude == 0 {
        0
    } else if magnitude > infinity {
        quiet_nan
    } else {
        bits
    }
}

/// Compares two byte sequences of `len` bytes for equality.
unsafe fn bytes_equal(a: *const u8, b: *const u8, len: usize) -> bool {
    if len == 0 {
        return true;
    }
    std::slice::from_raw_parts(a, len) == std::slice::from_raw_parts(b, len)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An identity registering `rule` through `boxed_depth` boxes over a
    /// pointer-wide value, ending in a nullable pointer when `nullable` says so.
    fn identity(rule: usize, boxed_depth: usize, nullable: bool) -> ElementIdentity {
        let mut identity = ElementIdentity::BYTES;
        identity.set_kind(through_optionals(rule, boxed_depth, 8, nullable));
        identity
    }

    /// The address of a slot holding `word`, as a container stores an element.
    fn slot(word: &usize) -> *const u8 {
        (word as *const usize).cast()
    }

    #[test]
    fn a_nullable_layer_hashes_none_apart_from_some_none() {
        let identity = identity(BY_BYTES, 1, true);
        let none = 0usize;
        let inner_none = 0usize;
        let some_none = &inner_none as *const usize as usize;
        unsafe {
            assert_ne!(
                identity.hash(slot(&none), 8),
                identity.hash(slot(&some_none), 8)
            );
            assert!(!identity.same(slot(&none), slot(&some_none), 8));
        }
    }

    #[test]
    fn a_nullable_layer_reads_its_pointer_word_without_opening_it() {
        let identity = identity(BY_BYTES, 0, true);
        let pointee = 7u64;
        let present = &pointee as *const u64 as usize;
        let none = 0usize;
        unsafe {
            assert!(matches!(
                identity.resolve(slot(&none)),
                Resolved::Missing(0)
            ));
            assert!(matches!(
                identity.resolve(slot(&present)),
                Resolved::Value(at) if at == slot(&present)
            ));
        }
    }

    #[test]
    fn a_boxed_encoding_without_the_nullable_bit_resolves_as_before() {
        let identity = identity(BY_BYTES, 1, false);
        let value = 0u64;
        let some_zero = &value as *const u64 as usize;
        unsafe {
            let boxed: *const u8 = (&value as *const u64).cast();
            assert!(matches!(
                identity.resolve(slot(&some_zero)),
                Resolved::Value(at) if at == boxed
            ));
        }
    }

    #[test]
    fn a_bare_kind_word_is_its_rule() {
        assert_eq!(
            through_optionals(BY_STRING_CONTENT, 0, 8, false) & 0xff,
            BY_STRING_CONTENT
        );
        assert_eq!(
            identity(BY_F64_VALUE, 3, true).rule(),
            BY_F64_VALUE,
            "the nullable bit does not leak into the rule"
        );
        assert_eq!(identity(BY_BYTES, 3, true).optional_depth(), 3);
    }
}
