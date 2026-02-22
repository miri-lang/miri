//! String implementation for Miri runtime.
//!
//! Provides a UTF-8 string type with C-compatible FFI interface.

use std::slice;
use std::str;

/// The memory layout of a Miri String as seen from both Rust and Miri code.
///
/// This struct is `#[repr(C)]` to ensure a stable, predictable memory layout
/// that can be shared across the FFI boundary.
///
/// Memory layout:
/// - `data`: Pointer to UTF-8 encoded bytes (not null-terminated)
/// - `len`: Number of bytes in the string
/// - `capacity`: Allocated capacity in bytes
#[repr(C)]
pub struct MiriString {
    pub data: *mut u8,
    pub len: usize,
    pub capacity: usize,
}

impl MiriString {
    /// Creates an empty MiriString.
    pub fn new() -> Self {
        Self {
            data: std::ptr::null_mut(),
            len: 0,
            capacity: 0,
        }
    }

    /// Creates a MiriString from a Rust &str.
    pub fn from_str(s: &str) -> Self {
        if s.is_empty() {
            return Self::new();
        }

        let bytes = s.as_bytes();
        let capacity = bytes.len();
        let data = unsafe { crate::alloc::miri_alloc(capacity, 1) };

        if data.is_null() {
            panic!("MiriString allocation failed");
        }

        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), data, capacity);
        }

        Self {
            data,
            len: bytes.len(),
            capacity,
        }
    }

    /// Returns the string as a Rust &str slice.
    ///
    /// # Safety
    /// The string data must be valid UTF-8.
    pub unsafe fn as_str(&self) -> &str {
        if self.data.is_null() || self.len == 0 {
            return "";
        }
        let bytes = slice::from_raw_parts(self.data, self.len);
        str::from_utf8_unchecked(bytes)
    }

    /// Returns the length in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns true if the string is empty.
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
            unsafe {
                crate::alloc::miri_free(self.data, self.capacity, 1);
            }
        }
    }
}

// =============================================================================
// FFI Functions - These are called from Miri code via intrinsics
// =============================================================================

/// Creates a new empty string.
///
/// Returns a pointer to a heap-allocated MiriString.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_new() -> *mut MiriString {
    let string = Box::new(MiriString::new());
    Box::into_raw(string)
}

/// Creates a string from raw UTF-8 bytes.
///
/// # Safety
/// - `data` must point to valid UTF-8 bytes.
/// - `len` must be the exact length of the data.
/// - The caller retains ownership of the `data` buffer (it is copied).
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_from_raw(data: *const u8, len: usize) -> *mut MiriString {
    if data.is_null() || len == 0 {
        return miri_rt_string_new();
    }

    let bytes = slice::from_raw_parts(data, len);
    let s = match str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return miri_rt_string_new(), // Invalid UTF-8, return empty
    };

    let string = Box::new(MiriString::from_str(s));
    Box::into_raw(string)
}

/// Returns the byte length of a string.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString` or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_len(ptr: *const MiriString) -> usize {
    if ptr.is_null() {
        return 0;
    }
    (*ptr).len
}

/// Returns the character (Unicode scalar) count of a string.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString` or null.
/// - The `MiriString` must contain valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_char_count(ptr: *const MiriString) -> usize {
    if ptr.is_null() {
        return 0;
    }
    let s = (*ptr).as_str();
    s.chars().count()
}

/// Returns true (1) if the string is empty, false (0) otherwise.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_is_empty(ptr: *const MiriString) -> u8 {
    if ptr.is_null() {
        return 1;
    }
    if (*ptr).is_empty() {
        1
    } else {
        0
    }
}

/// Concatenates two strings and returns a new string.
///
/// # Safety
/// - `left` and `right` must be valid pointers to `MiriString`s or null.
/// - If non-null, the `MiriString`s must contain valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_concat(
    left: *const MiriString,
    right: *const MiriString,
) -> *mut MiriString {
    let left_len = if left.is_null() { 0 } else { (*left).len };
    let right_len = if right.is_null() { 0 } else { (*right).len };

    if left_len == 0 && right_len == 0 {
        return miri_rt_string_new();
    }

    let total_len = left_len + right_len;
    // Check for overflow
    if total_len < left_len {
        return miri_rt_string_new(); // Fail on overflow
    }

    let data = crate::alloc::miri_alloc(total_len, 1);
    if data.is_null() {
        return miri_rt_string_new(); // Fail on allocation failure
    }

    if left_len > 0 {
        std::ptr::copy_nonoverlapping((*left).data, data, left_len);
    }
    if right_len > 0 {
        std::ptr::copy_nonoverlapping((*right).data, data.add(left_len), right_len);
    }

    let string = Box::new(MiriString {
        data,
        len: total_len,
        capacity: total_len,
    });
    Box::into_raw(string)
}

/// Converts a string to lowercase.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString` or null.
/// - The `MiriString` must contain valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_to_lower(ptr: *const MiriString) -> *mut MiriString {
    if ptr.is_null() {
        return miri_rt_string_new();
    }
    let s = (*ptr).as_str();
    let lowered = s.to_lowercase();
    let string = Box::new(MiriString::from_str(&lowered));
    Box::into_raw(string)
}

/// Converts a string to uppercase.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString` or null.
/// - The `MiriString` must contain valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_to_upper(ptr: *const MiriString) -> *mut MiriString {
    if ptr.is_null() {
        return miri_rt_string_new();
    }
    let s = (*ptr).as_str();
    let uppered = s.to_uppercase();
    let string = Box::new(MiriString::from_str(&uppered));
    Box::into_raw(string)
}

/// Trims whitespace from both ends of a string.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString` or null.
/// - The `MiriString` must contain valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_trim(ptr: *const MiriString) -> *mut MiriString {
    if ptr.is_null() {
        return miri_rt_string_new();
    }
    let s = (*ptr).as_str();
    let trimmed = s.trim();
    let string = Box::new(MiriString::from_str(trimmed));
    Box::into_raw(string)
}

/// Trims whitespace from the start of a string.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString` or null.
/// - The `MiriString` must contain valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_trim_start(ptr: *const MiriString) -> *mut MiriString {
    if ptr.is_null() {
        return miri_rt_string_new();
    }
    let s = (*ptr).as_str();
    let trimmed = s.trim_start();
    let string = Box::new(MiriString::from_str(trimmed));
    Box::into_raw(string)
}

/// Trims whitespace from the end of a string.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString` or null.
/// - The `MiriString` must contain valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_trim_end(ptr: *const MiriString) -> *mut MiriString {
    if ptr.is_null() {
        return miri_rt_string_new();
    }
    let s = (*ptr).as_str();
    let trimmed = s.trim_end();
    let string = Box::new(MiriString::from_str(trimmed));
    Box::into_raw(string)
}

/// Checks if a string contains a substring.
///
/// # Safety
/// - `haystack` and `needle` must be valid descriptors of `MiriString` or null.
/// - If non-null, strings must be valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_contains(
    haystack: *const MiriString,
    needle: *const MiriString,
) -> u8 {
    if haystack.is_null() || needle.is_null() {
        return 0;
    }
    let h = (*haystack).as_str();
    let n = (*needle).as_str();
    if h.contains(n) {
        1
    } else {
        0
    }
}

/// Checks if a string starts with a prefix.
///
/// # Safety
/// - `s` and `prefix` must be valid `MiriString` pointers or null.
/// - Strings must be valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_starts_with(
    s: *const MiriString,
    prefix: *const MiriString,
) -> u8 {
    if s.is_null() || prefix.is_null() {
        return 0;
    }
    let str_val = (*s).as_str();
    let prefix_val = (*prefix).as_str();
    if str_val.starts_with(prefix_val) {
        1
    } else {
        0
    }
}

/// Checks if a string ends with a suffix.
///
/// # Safety
/// - `s` and `suffix` must be valid `MiriString` pointers or null.
/// - Strings must be valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_ends_with(
    s: *const MiriString,
    suffix: *const MiriString,
) -> u8 {
    if s.is_null() || suffix.is_null() {
        return 0;
    }
    let str_val = (*s).as_str();
    let suffix_val = (*suffix).as_str();
    if str_val.ends_with(suffix_val) {
        1
    } else {
        0
    }
}

/// Replaces all occurrences of `from` with `to` in the string.
///
/// # Safety
/// - `s`, `from`, and `to` must be valid `MiriString` pointers or null.
/// - Strings must be valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_replace(
    s: *const MiriString,
    from: *const MiriString,
    to: *const MiriString,
) -> *mut MiriString {
    if s.is_null() {
        return miri_rt_string_new();
    }
    let str_val = (*s).as_str();
    let from_val = if from.is_null() { "" } else { (*from).as_str() };
    let to_val = if to.is_null() { "" } else { (*to).as_str() };

    if from_val.is_empty() {
        // Can't replace empty string, just clone
        let string = Box::new(MiriString::from_str(str_val));
        return Box::into_raw(string);
    }

    let replaced = str_val.replace(from_val, to_val);
    let string = Box::new(MiriString::from_str(&replaced));
    Box::into_raw(string)
}

/// Returns a substring given byte indices.
///
/// Returns an empty string if indices are out of bounds or invalid.
///
/// # Safety
/// - `s` must be a valid pointer to a `MiriString` or null.
/// - `start` and `end` must be valid byte indices into the string.
/// - The `MiriString` must contain valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_substring(
    s: *const MiriString,
    start: usize,
    end: usize,
) -> *mut MiriString {
    if s.is_null() {
        return miri_rt_string_new();
    }
    let str_val = (*s).as_str();

    if start > end || end > str_val.len() {
        return miri_rt_string_new();
    }

    // Ensure we're at valid UTF-8 boundaries
    if !str_val.is_char_boundary(start) || !str_val.is_char_boundary(end) {
        return miri_rt_string_new();
    }

    let substring = &str_val[start..end];
    let string = Box::new(MiriString::from_str(substring));
    Box::into_raw(string)
}

/// Compares two strings for equality.
///
/// # Safety
/// - `a` and `b` must be valid `MiriString` pointers or null.
/// - If non-null, strings must be valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_equals(a: *const MiriString, b: *const MiriString) -> u8 {
    let a_str = if a.is_null() { "" } else { (*a).as_str() };
    let b_str = if b.is_null() { "" } else { (*b).as_str() };
    if a_str == b_str {
        1
    } else {
        0
    }
}

/// Returns the raw data pointer for a string.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString` or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_data(ptr: *const MiriString) -> *const u8 {
    if ptr.is_null() {
        return std::ptr::null();
    }
    (*ptr).data
}

/// Frees a MiriString.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString` allocated by this runtime or null.
/// - The pointer must not be used after this call.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_free(ptr: *mut MiriString) {
    if !ptr.is_null() {
        let _ = Box::from_raw(ptr);
    }
}

/// Clones a MiriString.
///
/// # Safety
/// - `ptr` must be a valid pointer to a `MiriString` or null.
/// - The `MiriString` must contain valid UTF-8.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_string_clone(ptr: *const MiriString) -> *mut MiriString {
    if ptr.is_null() {
        return miri_rt_string_new();
    }
    let s = (*ptr).as_str();
    let string = Box::new(MiriString::from_str(s));
    Box::into_raw(string)
}

// =============================================================================
// Type-to-String Conversion Functions
// =============================================================================

/// Converts a 64-bit integer to its string representation.
#[no_mangle]
pub extern "C" fn miri_rt_int_to_string(value: i64) -> *mut MiriString {
    let s = value.to_string();
    let string = Box::new(MiriString::from_str(&s));
    Box::into_raw(string)
}

/// Converts a 64-bit float to its string representation.
#[no_mangle]
pub extern "C" fn miri_rt_float_to_string(value: f64) -> *mut MiriString {
    let s = if value.fract() == 0.0 && value.is_finite() {
        format!("{:.1}", value)
    } else {
        value.to_string()
    };
    let string = Box::new(MiriString::from_str(&s));
    Box::into_raw(string)
}

/// Converts a boolean (0 or 1) to its string representation ("true" or "false").
#[no_mangle]
pub extern "C" fn miri_rt_bool_to_string(value: i64) -> *mut MiriString {
    let s = if value != 0 { "true" } else { "false" };
    let string = Box::new(MiriString::from_str(s));
    Box::into_raw(string)
}
