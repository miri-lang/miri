//! Core `MiriString` type definition and inherent methods.

use std::slice;
use std::str;

/// The memory layout of a Miri string as seen from both Rust and compiled Miri code.
///
/// This struct is `#[repr(C)]` to ensure a stable, predictable memory layout
/// across the FFI boundary.
///
/// # Memory Layout
/// | Field      | Type       | Description                                          |
/// |------------|------------|------------------------------------------------------|
/// | `data`     | `*mut u8`  | Pointer to UTF-8 encoded bytes (not null-terminated) |
/// | `len`      | `usize`    | Number of bytes in the string                        |
/// | `capacity` | `usize`    | Allocated capacity in bytes                          |
///
/// # Ownership
/// The `data` pointer is allocated via [`crate::alloc::miri_alloc`] and freed
/// automatically when the `MiriString` is dropped. Empty strings use a null
/// `data` pointer with zero `len` and `capacity` (zero allocation).
#[repr(C)]
pub struct MiriString {
    pub data: *mut u8,
    pub len: usize,
    pub capacity: usize,
}

impl MiriString {
    /// Creates an empty `MiriString` with no heap allocation.
    #[must_use]
    pub fn new() -> Self {
        Self {
            data: std::ptr::null_mut(),
            len: 0,
            capacity: 0,
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
}

impl Default for MiriString {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for MiriString {
    fn drop(&mut self) {
        if !self.data.is_null() && self.capacity > 0 {
            // SAFETY: `data` was allocated via `miri_alloc` with alignment 1
            // and `self.capacity` bytes. It has not been freed yet.
            unsafe {
                crate::alloc::miri_free(self.data, self.capacity, 1);
            }
        }
    }
}
