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

/// How a container decides that two of its elements are the same element.
///
/// Every container starts [`BY_BYTES`] with no callback. The rule must be
/// settled while the container is still empty: an element already stored was
/// placed by the hash of the rule in force when it arrived.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct ElementIdentity {
    /// [`BY_BYTES`] or [`BY_STRING_CONTENT`]; consulted when `equals_fn` is zero.
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

    /// Selects the built-in rule, [`BY_BYTES`] or [`BY_STRING_CONTENT`].
    pub(crate) fn set_kind(&mut self, kind: usize) {
        self.kind = kind;
    }

    /// Routes equality through the element type's own `equals`.
    pub(crate) fn set_equals_fn(&mut self, equals_fn: usize) {
        self.equals_fn = equals_fn;
    }

    /// Hashes the element stored at `elem`, which spans `size` bytes.
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
    /// `elem` must point at `size` readable bytes holding an element of this rule.
    pub(crate) unsafe fn hash(&self, elem: *const u8, size: usize) -> u64 {
        if self.equals_fn != 0 {
            return 0;
        }
        if self.kind == BY_STRING_CONTENT {
            let (data, len) = string_content(elem);
            return crate::hash::fnv1a(data, len);
        }
        crate::hash::fnv1a(elem, size)
    }

    /// True when the elements stored at `a` and `b`, each `size` bytes, are the
    /// same element.
    ///
    /// # Safety
    ///
    /// `a` and `b` must each point at `size` readable bytes holding an element
    /// of this rule.
    pub(crate) unsafe fn same(&self, a: *const u8, b: *const u8, size: usize) -> bool {
        if self.equals_fn != 0 {
            return self.same_through_callback(a, b);
        }
        if self.kind == BY_STRING_CONTENT {
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
