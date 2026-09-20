// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Deciding whether two elements of a type-erased container are the same.
//!
//! A set asks it of its elements and a map of its keys, and both must answer
//! the way `==` does for the element type, or a membership test disagrees with
//! the comparison the program would write by hand. The runtime knows an element
//! only by its size, so the compiler tells it which of three rules applies:
//!
//! - an element whose bytes *are* its value — an integer, a float, a boolean —
//!   is the same element when its bytes are;
//! - an element whose bytes point at a string is the same element when the two
//!   strings hold the same content, wherever each was allocated;
//! - an element whose type defines its own equality is compared through a
//!   callback the compiler generates from that type's `equals`.
//!
//! An optional element applies none of these to itself: its bytes are the
//! address of its `Some` box, and two separately built `Some(2)`s hold that
//! same 2 at two addresses. So the compiler also says how many optionals wrap
//! the element and how wide the value inside is, and the rules above apply to
//! that value once every box has been opened. A `None` is the absence of one,
//! equal only to a `None` reached after opening as many boxes.
//!
//! Hashing follows the same rule, since two elements that compare equal have
//! to land in the same probe chain.

use crate::string::MiriString;

/// An equality over two element values, as the compiler generates it.
///
/// Each argument is the value the element slot holds — for a managed element
/// that is the pointer to the value, matching what the drop and clone callbacks
/// are handed. Returns nonzero when the two are the same element.
pub type ElementEqualsFn = unsafe extern "C" fn(*const u8, *const u8) -> u8;

/// The element's bytes are its value.
pub const BY_BYTES: usize = 0;

/// The element's bytes point at a `MiriString`, matched by its content.
pub const BY_STRING_CONTENT: usize = 1;

/// Width of the field holding the rule that settles a resolved value.
const RULE_BITS: u32 = 8;

/// Width of the field holding how many optionals wrap the element.
const DEPTH_BITS: u32 = 8;

/// How many optionals one element may be wrapped in and still be matched by
/// content. Nesting deeper than this keeps the byte rule: the compiler declines
/// to encode a depth that does not fit, rather than truncating one into a rule
/// that would open the wrong number of boxes.
pub const MAX_OPTIONAL_DEPTH: usize = (1 << DEPTH_BITS) - 1;

/// The kind word registering `rule` for a value reached by opening `depth`
/// optionals, each box holding `value_size` bytes. `depth` of zero is the
/// element itself and leaves `rule` exactly as the two bare constants spell it,
/// so a container whose elements are not optional encodes as it always did.
pub const fn through_optionals(rule: usize, depth: usize, value_size: usize) -> usize {
    rule | (depth << RULE_BITS) | (value_size << (RULE_BITS + DEPTH_BITS))
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
    /// The rule, the optional depth and the wrapped value's size, packed by
    /// [`through_optionals`]. The rule is consulted when `equals_fn` is zero.
    kind: usize,
    /// Address of an [`ElementEqualsFn`], or zero when the element type defines
    /// no equality of its own.
    equals_fn: usize,
}

impl ElementIdentity {
    /// The identity every new container starts with: compare by bytes.
    pub(crate) const BYTES: Self = Self {
        kind: BY_BYTES,
        equals_fn: 0,
    };

    /// Selects the rule, as packed by [`through_optionals`].
    pub(crate) fn set_kind(&mut self, kind: usize) {
        self.kind = kind;
    }

    /// Routes equality through the element type's own `equals`.
    pub(crate) fn set_equals_fn(&mut self, equals_fn: usize) {
        self.equals_fn = equals_fn;
    }

    /// The rule settling two resolved values.
    fn rule(&self) -> usize {
        self.kind & ((1 << RULE_BITS) - 1)
    }

    /// How many optionals wrap the element; zero when it is not optional.
    fn optional_depth(&self) -> usize {
        (self.kind >> RULE_BITS) & MAX_OPTIONAL_DEPTH
    }

    /// The number of bytes the settling rule reads, given the element slot's
    /// own `slot_size`. An optional's boxed value has a width of its own — an
    /// `int?` box holds eight bytes where an `i32?` box holds four, with the
    /// rest of the box never written — so only the element itself is read at
    /// the size of its slot.
    fn value_size(&self, slot_size: usize) -> usize {
        if self.optional_depth() == 0 {
            return slot_size;
        }
        self.kind >> (RULE_BITS + DEPTH_BITS)
    }

    /// Opens every optional wrapping the element stored at `elem`.
    ///
    /// Each box holds its value at offset zero, so opening one is reading the
    /// pointer and continuing at what it points to; a null pointer is a `None`
    /// and ends the walk at the depth it was found.
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
        Resolved::Value(at)
    }

    /// Hashes the element stored at `elem`, whose slot spans `slot_size` bytes.
    ///
    /// An element compared through a callback hashes to one constant: the
    /// element type states when two values are equal but not how to hash one,
    /// and any hash taken from the bytes would separate equal values.
    ///
    /// TODO: a set or map of such elements therefore probes every entry on each
    /// lookup. Hashing them in constant time needs the element type to supply
    /// a hash consistent with its `equals`, which no trait declares yet.
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
            return 0;
        }
        if self.rule() == BY_STRING_CONTENT {
            let (data, len) = string_content(at);
            return crate::hash::fnv1a(data, len);
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
        bytes_equal(a, b, size)
    }

    /// Asks the element type's `equals` about the two values the slots hold.
    ///
    /// The generated callback treats a null value as equal only to another null,
    /// so the runtime never hands the user's method a missing receiver.
    unsafe fn same_through_callback(&self, a: *const u8, b: *const u8) -> bool {
        let equals: ElementEqualsFn = std::mem::transmute(self.equals_fn);
        let left = *(a as *const *const u8);
        let right = *(b as *const *const u8);
        equals(left, right) != 0
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

/// Compares two byte sequences of `len` bytes for equality.
unsafe fn bytes_equal(a: *const u8, b: *const u8, len: usize) -> bool {
    if len == 0 {
        return true;
    }
    std::slice::from_raw_parts(a, len) == std::slice::from_raw_parts(b, len)
}
