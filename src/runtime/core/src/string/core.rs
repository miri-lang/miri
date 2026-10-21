// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Core `MiriString` type definition and inherent methods.

use std::slice;
use std::str;

/// Where a string's bytes live, and so what releasing the string frees.
///
/// Stored in the byte after [`MiriString::ascii`]. Compiled code lays a string
/// literal out with that byte zeroed, so a literal reads as
/// [`StringStorage::Unowned`] with no change to how literals are emitted.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StringStorage {
    /// Bytes the string does not own, such as a literal's static data. They
    /// are never freed.
    Unowned = 0,
    /// A buffer of `capacity` bytes from [`crate::alloc::miri_alloc`], freed
    /// with the string. An empty string has no buffer: null `data` and zero
    /// `capacity`.
    Heap = 1,
    /// `capacity` bytes that follow the header in the header's own block,
    /// released with the block.
    Inline = 2,
}

/// The memory layout of a Miri string as seen from both Rust and compiled Miri code.
///
/// This struct is `#[repr(C)]` to ensure a stable, predictable memory layout
/// across the FFI boundary. Compiled code reads `len` at one word in, and
/// writes literals with this exact layout.
///
/// # Memory Layout
/// | Field        | Type            | Offset    | Description                                    |
/// |--------------|-----------------|-----------|------------------------------------------------|
/// | `data`       | `*mut u8`       | 0         | UTF-8 bytes (not null-terminated)              |
/// | `len`        | `usize`         | 1 word    | Number of bytes in the string                  |
/// | `capacity`   | `usize`         | 2 words   | Bytes `data` has room for                      |
/// | `ascii`      | `bool`          | 3 words   | Every byte is ASCII, so character index = byte offset |
/// | `storage`    | `StringStorage` | 3 words+1 | Where the bytes live                           |
/// | `block_room` | `u8`            | 3 words+2 | Bytes the header's block holds past the header |
///
/// # Ownership
/// A heap string's buffer is freed when the `MiriString` is dropped; an
/// inline string's bytes go with its block; an unowned string frees nothing.
/// A header with inline bytes is only ever built in place, in its block (see
/// `super::alloc_inline`): moving it would leave `data` pointing at the old
/// place.
#[repr(C)]
pub struct MiriString {
    pub data: *mut u8,
    pub len: usize,
    pub capacity: usize,
    /// Every byte is ASCII: each character is one byte, so a character index
    /// is a byte offset and the character count is the byte length, with no
    /// walk over the UTF-8. Settled when the string is built, and kept true
    /// by an append only of ASCII bytes.
    pub ascii: bool,
    pub storage: StringStorage,
    /// How many bytes the block holding this header was allocated with past
    /// the header. Set when the block is made and never changed, so the block
    /// is released at its own size even after an append has moved inline
    /// bytes out to a heap buffer. Zero for every block but an inline one.
    pub block_room: u8,
}

impl MiriString {
    /// Creates an empty `MiriString` with no heap allocation.
    #[must_use]
    pub fn new() -> Self {
        Self {
            data: std::ptr::null_mut(),
            len: 0,
            capacity: 0,
            ascii: true,
            storage: StringStorage::Heap,
            block_room: 0,
        }
    }

    /// Creates a `MiriString` by copying the bytes from a Rust `&str`.
    ///
    /// Returns an empty `MiriString` if the input is empty or if allocation fails.
    ///
    /// Named `from_str` for ergonomics; `FromStr` trait is not implemented because
    /// this constructor is infallible (returns empty on failure, never errors).
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Self {
        Self::from_str_of_ascii(s, s.is_ascii())
    }

    /// As [`Self::from_str`], for text already known to be ASCII or not —
    /// a piece of a string whose own mark says so — so it is not scanned
    /// again.
    #[must_use]
    pub(crate) fn from_str_of_ascii(s: &str, ascii: bool) -> Self {
        if s.is_empty() {
            return Self::new();
        }

        let bytes = s.as_bytes();
        let capacity = bytes.len();

        // SAFETY: `capacity` is non-zero (checked above) and alignment 1 is always valid.
        let data = unsafe { crate::alloc::miri_alloc(capacity, 1) };
        if data.is_null() {
            return Self::new();
        }

        // SAFETY: `data` is a freshly-allocated buffer of `capacity` bytes,
        // and `bytes` has exactly `capacity` bytes. The regions do not overlap.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, capacity);
        }

        Self {
            data,
            len: bytes.len(),
            capacity,
            ascii,
            storage: StringStorage::Heap,
            block_room: 0,
        }
    }

    /// Returns the string content as a Rust `&str` slice.
    ///
    /// Returns `""` for empty or null-data strings.
    ///
    /// # Safety
    /// The `data` buffer must contain valid UTF-8 bytes for its entire `len`.
    #[must_use]
    pub unsafe fn as_str(&self) -> &str {
        debug_assert!(
            self.is_in_place(),
            "an inline string was moved out of its block"
        );
        if self.data.is_null() || self.len == 0 {
            return "";
        }
        // SAFETY: Caller guarantees UTF-8 validity. `data` is non-null and
        // points to at least `self.len` allocated bytes.
        let bytes = slice::from_raw_parts(self.data, self.len);
        str::from_utf8_unchecked(bytes)
    }

    /// Returns the byte length of the string.
    #[must_use]
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if the string contains no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The payload size of the block holding this header — the header and
    /// the bytes its block holds past it — which the block is released at.
    #[must_use]
    pub(crate) fn block_payload_size(&self) -> usize {
        std::mem::size_of::<Self>() + usize::from(self.block_room)
    }

    /// True unless this is an inline string whose `data` does not point just
    /// past its own header, which would mean the header was moved out of its
    /// block.
    #[must_use]
    pub(crate) fn is_in_place(&self) -> bool {
        match self.storage {
            StringStorage::Inline => std::ptr::eq(
                self.data,
                (self as *const Self as *const u8).wrapping_add(std::mem::size_of::<Self>()),
            ),
            StringStorage::Heap | StringStorage::Unowned => true,
        }
    }
}

impl Default for MiriString {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MiriString {
    fn drop(&mut self) {
        super::char_index::forget(self);
        match self.storage {
            StringStorage::Heap => {
                if !self.data.is_null() && self.capacity > 0 {
                    // SAFETY: `data` was allocated via `miri_alloc` with
                    // alignment 1 and `self.capacity` bytes, and is not yet
                    // freed.
                    unsafe { crate::alloc::miri_free(self.data, self.capacity, 1) };
                }
            }
            // Inline bytes are released with the block that holds them, and
            // unowned bytes are never released.
            StringStorage::Inline | StringStorage::Unowned => {}
        }
    }
}
