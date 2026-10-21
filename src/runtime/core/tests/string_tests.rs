// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_core::rc::{alloc_with_rc, free_with_rc, RC_HEADER_SIZE};
use miri_runtime_core::string::*;
use miri_runtime_core::{miri_free, miri_rt_list_free, miri_rt_list_get, miri_rt_list_len};
use std::ptr;

/// Helper: create a `*mut MiriString` via the FFI constructor (RC layout).
unsafe fn make_str(s: &str) -> *mut MiriString {
    miri_rt_string_from_raw(s.as_ptr(), s.len())
}

// MiriString inherent methods

#[test]
fn test_string_creation() {
    let s = MiriString::from_str("Hello, World!");
    unsafe {
        assert_eq!(s.as_str(), "Hello, World!");
        assert_eq!(s.len(), 13);
    }
}

#[test]
fn test_empty_string() {
    let s = MiriString::new();
    unsafe {
        assert_eq!(s.as_str(), "");
        assert!(s.is_empty());
    }
}

#[test]
fn test_from_str_empty_returns_empty() {
    let s = MiriString::from_str("");
    assert!(s.is_empty());
    assert!(s.data.is_null());
}

#[test]
fn test_unicode() {
    let s = MiriString::from_str("こんにちは");
    unsafe {
        assert_eq!(s.as_str(), "こんにちは");
        assert_eq!(miri_rt_string_char_count(&s as *const _), 5);
        assert_eq!(s.len(), 15); // 3 bytes per character
    }
}

// Constructor FFI functions

#[test]
fn test_ffi_string_new() {
    unsafe {
        let s = miri_rt_string_new();
        assert!(!s.is_null());
        assert!((*s).is_empty());
        miri_rt_string_free(s);
    }
}

#[test]
fn test_ffi_string_from_raw() {
    unsafe {
        let data = b"hello";
        let s = miri_rt_string_from_raw(data.as_ptr(), data.len());
        assert_eq!((*s).as_str(), "hello");
        miri_rt_string_free(s);
    }
}

#[test]
fn test_ffi_string_from_raw_null() {
    unsafe {
        let s = miri_rt_string_from_raw(ptr::null(), 5);
        assert!((*s).is_empty());
        miri_rt_string_free(s);
    }
}

#[test]
fn test_ffi_string_from_raw_invalid_utf8() {
    unsafe {
        let data: [u8; 3] = [0xFF, 0xFE, 0xFD];
        let s = miri_rt_string_from_raw(data.as_ptr(), data.len());
        assert!((*s).is_empty());
        miri_rt_string_free(s);
    }
}

#[test]
fn test_ffi_string_clone() {
    unsafe {
        let original = make_str("clone me");
        let cloned = miri_rt_string_clone(original);
        assert_eq!((*cloned).as_str(), "clone me");
        // Verify it's a true deep copy — different pointers
        assert_ne!((*original).data, (*cloned).data);
        miri_rt_string_free(original);
        miri_rt_string_free(cloned);
    }
}

#[test]
fn test_ffi_string_clone_null() {
    unsafe {
        let cloned = miri_rt_string_clone(ptr::null());
        assert!((*cloned).is_empty());
        miri_rt_string_free(cloned);
    }
}

#[test]
fn test_ffi_free_null_is_noop() {
    unsafe {
        miri_rt_string_free(ptr::null_mut()); // must not crash
    }
}

// Inspection FFI functions

#[test]
fn test_ffi_string_len() {
    unsafe {
        let s = make_str("hello");
        assert_eq!(miri_rt_string_len(s), 5);
        assert_eq!(miri_rt_string_len(ptr::null()), 0);
        miri_rt_string_free(s);
    }
}

#[test]
fn test_ffi_string_is_empty() {
    unsafe {
        let empty = miri_rt_string_new();
        let nonempty = make_str("x");
        assert_eq!(miri_rt_string_is_empty(empty), 1);
        assert_eq!(miri_rt_string_is_empty(nonempty), 0);
        assert_eq!(miri_rt_string_is_empty(ptr::null()), 1);
        miri_rt_string_free(empty);
        miri_rt_string_free(nonempty);
    }
}

#[test]
fn test_ffi_contains() {
    unsafe {
        let haystack = make_str("Hello, World!");
        let needle = make_str("World");
        let missing = make_str("xyz");

        assert_eq!(miri_rt_string_contains(haystack, needle), 1);
        assert_eq!(miri_rt_string_contains(haystack, missing), 0);
        assert_eq!(miri_rt_string_contains(haystack, ptr::null()), 0);
        assert_eq!(miri_rt_string_contains(ptr::null(), needle), 0);

        miri_rt_string_free(haystack);
        miri_rt_string_free(needle);
        miri_rt_string_free(missing);
    }
}

#[test]
fn test_ffi_starts_with() {
    unsafe {
        let s = make_str("Hello, World!");
        let prefix = make_str("Hello");
        let wrong = make_str("World");

        assert_eq!(miri_rt_string_starts_with(s, prefix), 1);
        assert_eq!(miri_rt_string_starts_with(s, wrong), 0);
        assert_eq!(miri_rt_string_starts_with(ptr::null(), prefix), 0);

        miri_rt_string_free(s);
        miri_rt_string_free(prefix);
        miri_rt_string_free(wrong);
    }
}

#[test]
fn test_ffi_ends_with() {
    unsafe {
        let s = make_str("Hello, World!");
        let suffix = make_str("World!");
        let wrong = make_str("Hello");

        assert_eq!(miri_rt_string_ends_with(s, suffix), 1);
        assert_eq!(miri_rt_string_ends_with(s, wrong), 0);
        assert_eq!(miri_rt_string_ends_with(ptr::null(), suffix), 0);

        miri_rt_string_free(s);
        miri_rt_string_free(suffix);
        miri_rt_string_free(wrong);
    }
}

#[test]
fn test_ffi_equals() {
    unsafe {
        let a = make_str("same");
        let b = make_str("same");
        let c = make_str("different");

        assert_eq!(miri_rt_string_equals(a, b), 1);
        assert_eq!(miri_rt_string_equals(a, c), 0);
        // Two nulls are equal (both empty)
        assert_eq!(miri_rt_string_equals(ptr::null(), ptr::null()), 1);

        miri_rt_string_free(a);
        miri_rt_string_free(b);
        miri_rt_string_free(c);
    }
}

#[test]
fn test_ffi_string_data() {
    unsafe {
        let s = make_str("abc");
        let data = miri_rt_string_data(s);
        assert!(!data.is_null());
        assert_eq!(*data, b'a');
        assert_eq!(miri_rt_string_data(ptr::null()), ptr::null());
        miri_rt_string_free(s);
    }
}

// Transformation FFI functions

#[test]
fn test_ffi_concat() {
    unsafe {
        let a = make_str("Hello, ");
        let b = make_str("World!");

        let result = miri_rt_string_concat(a, b);
        assert_eq!((*result).as_str(), "Hello, World!");

        miri_rt_string_free(a);
        miri_rt_string_free(b);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_concat_null_operands() {
    unsafe {
        let s = make_str("hello");

        let result_left_null = miri_rt_string_concat(ptr::null(), s);
        assert_eq!((*result_left_null).as_str(), "hello");

        let result_right_null = miri_rt_string_concat(s, ptr::null());
        assert_eq!((*result_right_null).as_str(), "hello");

        let result_both_null = miri_rt_string_concat(ptr::null(), ptr::null());
        assert!((*result_both_null).is_empty());

        miri_rt_string_free(s);
        miri_rt_string_free(result_left_null);
        miri_rt_string_free(result_right_null);
        miri_rt_string_free(result_both_null);
    }
}

#[test]
fn test_ffi_to_lower() {
    unsafe {
        let s = make_str("HELLO");
        let result = miri_rt_string_to_lower(s);
        assert_eq!((*result).as_str(), "hello");

        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_to_upper() {
    unsafe {
        let s = make_str("hello");
        let result = miri_rt_string_to_upper(s);
        assert_eq!((*result).as_str(), "HELLO");

        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_trim() {
    unsafe {
        let s = make_str("  hello  ");
        let result = miri_rt_string_trim(s);
        assert_eq!((*result).as_str(), "hello");
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_trim_start() {
    unsafe {
        let s = make_str("  hello  ");
        let result = miri_rt_string_trim_start(s);
        assert_eq!((*result).as_str(), "hello  ");
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_trim_end() {
    unsafe {
        let s = make_str("  hello  ");
        let result = miri_rt_string_trim_end(s);
        assert_eq!((*result).as_str(), "  hello");
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_replace() {
    unsafe {
        let s = make_str("hello world hello");
        let from = make_str("hello");
        let to = make_str("hi");

        let result = miri_rt_string_replace(s, from, to);
        assert_eq!((*result).as_str(), "hi world hi");

        miri_rt_string_free(s);
        miri_rt_string_free(from);
        miri_rt_string_free(to);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_replace_empty_from_returns_copy() {
    unsafe {
        let s = make_str("hello");
        let empty = miri_rt_string_new();
        let to = make_str("x");

        let result = miri_rt_string_replace(s, empty, to);
        assert_eq!((*result).as_str(), "hello");

        miri_rt_string_free(s);
        miri_rt_string_free(empty);
        miri_rt_string_free(to);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_substring() {
    unsafe {
        let s = make_str("Hello, World!");
        let sub = miri_rt_string_substring(s, 7, 12);
        assert_eq!((*sub).as_str(), "World");

        // An empty range inside the text is an empty string. A range past the
        // end, or a reversed one, ends the process with a runtime error, so
        // the integration suite covers those (tests/integration/strings).
        let empty = miri_rt_string_substring(s, 5, 5);
        assert!((*empty).is_empty());

        miri_rt_string_free(s);
        miri_rt_string_free(sub);
        miri_rt_string_free(empty);
    }
}

#[test]
fn test_ffi_substring_counts_characters_not_bytes() {
    unsafe {
        // "café" is five bytes, 'é' taking two, but four characters.
        let s = make_str("café");
        let whole = miri_rt_string_substring(s, 0, 4);
        assert_eq!((*whole).as_str(), "café");

        let accented = miri_rt_string_substring(s, 3, 4);
        assert_eq!((*accented).as_str(), "é");

        let head = miri_rt_string_substring(s, 0, 3);
        assert_eq!((*head).as_str(), "caf");

        miri_rt_string_free(s);
        miri_rt_string_free(whole);
        miri_rt_string_free(accented);
        miri_rt_string_free(head);
    }
}

#[test]
fn test_ffi_char_at() {
    unsafe {
        let s = make_str("hello");
        let ch = miri_rt_string_char_at(s, 1);
        assert_eq!((*ch).as_str(), "e");

        // Out of bounds
        let oob = miri_rt_string_char_at(s, 100);
        assert!((*oob).is_empty());

        miri_rt_string_free(s);
        miri_rt_string_free(ch);
        miri_rt_string_free(oob);
    }
}

#[test]
fn test_ffi_char_at_unicode() {
    unsafe {
        let s = make_str("こんにちは");
        let ch = miri_rt_string_char_at(s, 2);
        assert_eq!((*ch).as_str(), "に");
        miri_rt_string_free(s);
        miri_rt_string_free(ch);
    }
}

#[test]
fn test_ffi_repeat() {
    unsafe {
        let s = make_str("ab");
        let result = miri_rt_string_repeat(s, 3);
        assert_eq!((*result).as_str(), "ababab");

        let zero = miri_rt_string_repeat(s, 0);
        assert!((*zero).is_empty());

        miri_rt_string_free(s);
        miri_rt_string_free(result);
        miri_rt_string_free(zero);
    }
}

// Type conversion FFI functions

#[test]
fn test_ffi_int_to_string() {
    unsafe {
        let pos = miri_rt_int_to_string(42);
        assert_eq!((*pos).as_str(), "42");

        let neg = miri_rt_int_to_string(-7);
        assert_eq!((*neg).as_str(), "-7");

        let zero = miri_rt_int_to_string(0);
        assert_eq!((*zero).as_str(), "0");

        miri_rt_string_free(pos);
        miri_rt_string_free(neg);
        miri_rt_string_free(zero);
    }
}

#[test]
fn test_ffi_float_to_string() {
    unsafe {
        // Whole number gets .1 formatting
        let whole = miri_rt_float_to_string(3.0);
        assert_eq!((*whole).as_str(), "3.0");

        // Fractional keeps natural formatting
        let frac = miri_rt_float_to_string(3.25);
        assert_eq!((*frac).as_str(), "3.25");

        let neg = miri_rt_float_to_string(-0.5);
        assert_eq!((*neg).as_str(), "-0.5");

        miri_rt_string_free(whole);
        miri_rt_string_free(frac);
        miri_rt_string_free(neg);
    }
}

#[test]
fn test_ffi_bool_to_string() {
    unsafe {
        let t = miri_rt_bool_to_string(1);
        assert_eq!((*t).as_str(), "true");

        let f = miri_rt_bool_to_string(0);
        assert_eq!((*f).as_str(), "false");

        // Any non-zero is true
        let also_true = miri_rt_bool_to_string(42);
        assert_eq!((*also_true).as_str(), "true");

        miri_rt_string_free(t);
        miri_rt_string_free(f);
        miri_rt_string_free(also_true);
    }
}

// Edge cases for conversions

#[test]
fn test_ffi_int_to_string_extremes() {
    unsafe {
        let max = miri_rt_int_to_string(i64::MAX);
        assert_eq!((*max).as_str(), "9223372036854775807");

        let min = miri_rt_int_to_string(i64::MIN);
        assert_eq!((*min).as_str(), "-9223372036854775808");

        miri_rt_string_free(max);
        miri_rt_string_free(min);
    }
}

#[test]
fn test_ffi_float_to_string_special() {
    unsafe {
        let nan = miri_rt_float_to_string(f64::NAN);
        assert_eq!((*nan).as_str(), "NaN");

        let inf = miri_rt_float_to_string(f64::INFINITY);
        assert_eq!((*inf).as_str(), "inf");

        let neg_inf = miri_rt_float_to_string(f64::NEG_INFINITY);
        assert_eq!((*neg_inf).as_str(), "-inf");

        let neg_zero = miri_rt_float_to_string(-0.0);
        // Rust preserves the negative sign on -0.0
        assert_eq!((*neg_zero).as_str(), "-0.0");

        let large = miri_rt_float_to_string(1e20);
        assert!(!(*large).is_empty());

        miri_rt_string_free(nan);
        miri_rt_string_free(inf);
        miri_rt_string_free(neg_inf);
        miri_rt_string_free(neg_zero);
        miri_rt_string_free(large);
    }
}

#[test]
fn test_ffi_bool_to_string_negative() {
    unsafe {
        let neg = miri_rt_bool_to_string(-1);
        assert_eq!((*neg).as_str(), "true");
        miri_rt_string_free(neg);
    }
}

// Additional string edge cases

#[test]
fn test_ffi_concat_empty_strings() {
    unsafe {
        let a = miri_rt_string_new();
        let b = miri_rt_string_new();

        let result = miri_rt_string_concat(a, b);
        assert!((*result).is_empty());

        miri_rt_string_free(a);
        miri_rt_string_free(b);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_concat_unicode() {
    unsafe {
        let a = make_str("Hello ");
        let b = make_str("世界!");

        let result = miri_rt_string_concat(a, b);
        assert_eq!((*result).as_str(), "Hello 世界!");

        miri_rt_string_free(a);
        miri_rt_string_free(b);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_to_lower_unicode() {
    unsafe {
        let s = make_str("CAFÉ");
        let result = miri_rt_string_to_lower(s);
        assert_eq!((*result).as_str(), "café");
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_to_upper_unicode() {
    unsafe {
        let s = make_str("café");
        let result = miri_rt_string_to_upper(s);
        assert_eq!((*result).as_str(), "CAFÉ");
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_trim_tabs_and_newlines() {
    unsafe {
        let s = make_str("\t\n hello \r\n");
        let result = miri_rt_string_trim(s);
        assert_eq!((*result).as_str(), "hello");
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_trim_all_whitespace() {
    unsafe {
        let s = make_str("   ");
        let result = miri_rt_string_trim(s);
        assert!((*result).is_empty());
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_trim_empty() {
    unsafe {
        let s = miri_rt_string_new();
        let result = miri_rt_string_trim(s);
        assert!((*result).is_empty());
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_trim_null() {
    unsafe {
        let result = miri_rt_string_trim(ptr::null());
        assert!((*result).is_empty());
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_replace_null_args() {
    unsafe {
        // Null source
        let from = make_str("x");
        let to = make_str("y");
        let result = miri_rt_string_replace(ptr::null(), from, to);
        assert!((*result).is_empty());
        miri_rt_string_free(from);
        miri_rt_string_free(to);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_replace_null_to() {
    unsafe {
        let s = make_str("hello world");
        let from = make_str("world");
        let result = miri_rt_string_replace(s, from, ptr::null());
        assert_eq!((*result).as_str(), "hello ");
        miri_rt_string_free(s);
        miri_rt_string_free(from);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_substring_full_range() {
    unsafe {
        let s = make_str("hello");

        // Full string
        let full = miri_rt_string_substring(s, 0, 5);
        assert_eq!((*full).as_str(), "hello");

        // Empty range at start
        let empty = miri_rt_string_substring(s, 0, 0);
        assert!((*empty).is_empty());

        // Null
        let null_sub = miri_rt_string_substring(ptr::null(), 0, 5);
        assert!((*null_sub).is_empty());

        miri_rt_string_free(s);
        miri_rt_string_free(full);
        miri_rt_string_free(empty);
        miri_rt_string_free(null_sub);
    }
}

#[test]
fn test_ffi_char_at_null() {
    unsafe {
        let result = miri_rt_string_char_at(ptr::null(), 0);
        assert!((*result).is_empty());
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_repeat_one() {
    unsafe {
        let s = make_str("abc");
        let result = miri_rt_string_repeat(s, 1);
        assert_eq!((*result).as_str(), "abc");
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_repeat_null() {
    unsafe {
        let result = miri_rt_string_repeat(ptr::null(), 5);
        assert!((*result).is_empty());
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_contains_empty_needle() {
    unsafe {
        let haystack = make_str("hello");
        let empty = miri_rt_string_new();

        // In Rust, "hello".contains("") is true
        assert_eq!(miri_rt_string_contains(haystack, empty), 1);

        miri_rt_string_free(haystack);
        miri_rt_string_free(empty);
    }
}

#[test]
fn test_ffi_starts_with_empty() {
    unsafe {
        let s = make_str("hello");
        let empty = miri_rt_string_new();

        // "hello".starts_with("") is true in Rust
        assert_eq!(miri_rt_string_starts_with(s, empty), 1);

        miri_rt_string_free(s);
        miri_rt_string_free(empty);
    }
}

#[test]
fn test_ffi_ends_with_empty() {
    unsafe {
        let s = make_str("hello");
        let empty = miri_rt_string_new();

        assert_eq!(miri_rt_string_ends_with(s, empty), 1);

        miri_rt_string_free(s);
        miri_rt_string_free(empty);
    }
}

#[test]
fn test_ffi_equals_null_vs_empty() {
    unsafe {
        let empty = miri_rt_string_new();
        // null treated as empty, so null == empty
        assert_eq!(miri_rt_string_equals(ptr::null(), empty), 1);
        assert_eq!(miri_rt_string_equals(empty, ptr::null()), 1);
        miri_rt_string_free(empty);
    }
}

#[test]
fn test_ffi_string_char_count_ascii() {
    unsafe {
        let s = make_str("hello");
        assert_eq!(miri_rt_string_char_count(s), 5);
        miri_rt_string_free(s);
    }
}

#[test]
fn test_ffi_string_char_count_empty() {
    unsafe {
        let s = miri_rt_string_new();
        assert_eq!(miri_rt_string_char_count(s), 0);
        assert_eq!(miri_rt_string_char_count(ptr::null()), 0);
        miri_rt_string_free(s);
    }
}

#[test]
fn test_ffi_string_char_count_emoji() {
    unsafe {
        let s = make_str("Hi! 👋🌍");
        // 'H', 'i', '!', ' ', '👋', '🌍' = 6 chars
        assert_eq!(miri_rt_string_char_count(s), 6);
        miri_rt_string_free(s);
    }
}

#[test]
fn test_string_default() {
    let s = MiriString::default();
    assert!(s.is_empty());
    assert!(s.data.is_null());
}

#[test]
fn test_ffi_string_from_raw_zero_len() {
    unsafe {
        let data = b"hello";
        let s = miri_rt_string_from_raw(data.as_ptr(), 0);
        assert!((*s).is_empty());
        miri_rt_string_free(s);
    }
}

/// Joining and appending keep a string marked ASCII exactly while every byte
/// it holds is ASCII.
#[test]
fn test_ascii_mark_follows_concat_and_append() {
    unsafe {
        let ascii = miri_rt_string_from_raw("abc".as_ptr(), 3);
        let accented = miri_rt_string_from_raw("é".as_ptr(), "é".len());
        assert!((*ascii).ascii);
        assert!(!(*accented).ascii);

        let joined = miri_rt_string_concat(ascii, ascii);
        assert!((*joined).ascii);
        let mixed = miri_rt_string_concat(ascii, accented);
        assert!(!(*mixed).ascii);

        let grown = miri_rt_string_append(joined, ascii);
        assert!((*grown).ascii, "an ASCII append keeps the mark");
        let grown = miri_rt_string_append(grown, accented);
        assert!(!(*grown).ascii, "a non-ASCII append clears it");
        assert_eq!((*grown).as_str(), "abcabcabcé");

        miri_rt_string_free(grown);
        miri_rt_string_free(mixed);
        miri_rt_string_free(accented);
        miri_rt_string_free(ascii);
    }
}

// Short strings keep their bytes in their header's block

/// Where an inline string's bytes start: right after its header.
fn past_header(string: *const MiriString) -> *mut u8 {
    (string as *mut u8).wrapping_add(std::mem::size_of::<MiriString>())
}

/// True when `string` keeps its bytes in its own block, right after its
/// header, with room for exactly its text.
unsafe fn is_inline(string: *const MiriString) -> bool {
    (*string).storage == StringStorage::Inline
        && (*string).data == past_header(string)
        && (*string).capacity == (*string).len
}

/// The pieces `text` splits into at `separator`, each owned by the caller.
/// The list that held them is released without them.
unsafe fn split_pieces(text: &str, separator: &str) -> Vec<*mut MiriString> {
    let text = make_str(text);
    let separator = make_str(separator);
    let list = miri_runtime_core::string::ffi::miri_rt_string_split(text, separator);
    let pieces = (0..miri_rt_list_len(list))
        .map(|i| *(miri_rt_list_get(list, i) as *const *mut MiriString))
        .collect();
    miri_rt_list_free(list);
    miri_rt_string_free(separator);
    miri_rt_string_free(text);
    pieces
}

#[test]
fn test_inline_block_fits_the_largest_cached_class() {
    assert!(
        RC_HEADER_SIZE + std::mem::size_of::<MiriString>() + INLINE_CAPACITY
            <= miri_runtime_core::block_cache::LARGEST_CACHED_BLOCK
    );
}

#[test]
fn test_split_pieces_keep_their_bytes_in_their_own_block() {
    unsafe {
        let pieces = split_pieces("a,bb,ccc", ",");
        let texts: Vec<&str> = pieces.iter().map(|&piece| (*piece).as_str()).collect();
        assert_eq!(texts, ["a", "bb", "ccc"]);
        for &piece in &pieces {
            assert!(is_inline(piece), "{:?} is not inline", (*piece).as_str());
        }
        pieces
            .into_iter()
            .for_each(|piece| miri_rt_string_free(piece));
    }
}

/// Every producer of a fresh short string makes it inline.
#[test]
fn test_short_strings_made_from_other_strings_are_inline() {
    unsafe {
        let text = make_str("  Hello, wörld  ");
        let short = make_str("ab");
        let produced = [
            miri_rt_string_substring(text, 2, 7),
            miri_rt_string_trim(text),
            miri_rt_string_trim_start(text),
            miri_rt_string_trim_end(text),
            miri_rt_string_char_at(text, 10),
            miri_rt_string_to_upper(text),
            miri_rt_string_to_lower(text),
            miri_rt_string_replace(text, short, short),
            miri_rt_string_repeat(short, 3),
            miri_rt_string_concat(short, text),
            miri_rt_string_clone(text),
            miri_rt_int_to_string(-42),
            miri_rt_float_to_string(2.5),
            miri_rt_bool_to_string(1),
            short,
        ];
        for &string in &produced {
            assert!(is_inline(string), "{:?} is not inline", (*string).as_str());
        }
        assert_eq!((*produced[0]).as_str(), "Hello");
        assert_eq!((*produced[4]).as_str(), "ö");
        produced
            .into_iter()
            .for_each(|string| miri_rt_string_free(string));
        miri_rt_string_free(text);
    }
}

#[test]
fn test_text_longer_than_the_inline_capacity_uses_a_heap_buffer() {
    unsafe {
        let longest = "x".repeat(INLINE_CAPACITY);
        let padded = make_str(&format!(" {longest}y "));
        let fits = miri_rt_string_substring(padded, 1, 1 + INLINE_CAPACITY as isize);
        assert!(is_inline(fits));
        let over = miri_rt_string_substring(padded, 1, 2 + INLINE_CAPACITY as isize);
        let trimmed = miri_rt_string_trim(padded);
        for string in [over, trimmed] {
            assert_eq!((*string).storage, StringStorage::Heap);
            assert_ne!((*string).data, past_header(string));
            assert_eq!((*string).as_str(), format!("{longest}y"));
        }
        [fits, over, trimmed, padded]
            .into_iter()
            .for_each(|string| miri_rt_string_free(string));
    }
}

/// A released inline block comes back at the same address with the same data
/// pointer; what was remembered about the characters of the text it held
/// must not answer for the new text.
#[test]
fn test_reused_inline_block_counts_the_characters_of_its_new_text() {
    unsafe {
        let accented = make_str("héllo, there");
        let japanese = make_str("日本 and more");
        let first = miri_rt_string_substring(accented, 0, 5);
        assert_eq!(miri_rt_string_char_count(first), 5);
        miri_rt_string_free(first);

        let second = miri_rt_string_substring(japanese, 0, 2);
        assert_eq!(second, first, "the inline block was not reused");
        assert!(is_inline(second));
        assert_eq!((*second).len, 6, "both texts are six bytes");
        assert_eq!(miri_rt_string_char_count(second), 2);
        let second_char = miri_rt_string_char_at(second, 1);
        assert_eq!((*second_char).as_str(), "本");
        [second_char, second, japanese, accented]
            .into_iter()
            .for_each(|string| miri_rt_string_free(string));
    }
}

/// Appending to an inline string nothing else holds moves its text into a
/// heap buffer that can grow, in the same header; the block is still released
/// at the size it was allocated with, so the next request of that size gets
/// it back. Twenty bytes of room put the block a class above a bare header's.
#[test]
fn test_appending_to_an_inline_string_moves_its_text_to_a_heap_buffer() {
    unsafe {
        let text = make_str("abcdefghijklmnopqrst and more");
        let piece = miri_rt_string_substring(text, 0, 20);
        assert!(is_inline(piece));
        let suffix = make_str("!");
        let grown = miri_rt_string_append(piece, suffix);
        assert_eq!(grown, piece, "an unshared string is appended in place");
        assert_eq!((*grown).storage, StringStorage::Heap);
        assert_eq!((*grown).as_str(), "abcdefghijklmnopqrst!");
        let mut grown = grown;
        for _ in 0..300 {
            grown = miri_rt_string_append(grown, suffix);
        }
        assert_eq!((*grown).len, 321);
        assert!((*grown).as_str().starts_with("abcdefghijklmnopqrst!!"));

        miri_rt_string_free(grown);
        let payload = std::mem::size_of::<MiriString>() + 20;
        let reused = alloc_with_rc(payload);
        assert_eq!(
            reused, grown as *mut u8,
            "the block went back at another size"
        );
        free_with_rc(reused, payload);
        miri_rt_string_free(suffix);
        miri_rt_string_free(text);
    }
}

/// A header laid out the way compiled code lays out a string literal, whose
/// bytes the string does not own, with reference count `count`.
struct LiteralShaped {
    words: [usize; 5],
}

impl LiteralShaped {
    fn new(text: &'static str, count: usize) -> Self {
        let mut words = [count, text.as_ptr() as usize, text.len(), text.len(), 0];
        words[4] = usize::from(text.is_ascii());
        Self { words }
    }

    fn string(&mut self) -> *mut MiriString {
        self.words[1..].as_mut_ptr() as *mut MiriString
    }
}

#[test]
fn test_literal_shaped_header_reads_as_unowned() {
    let mut literal = LiteralShaped::new("lit", 1usize << 63);
    unsafe {
        let string = literal.string();
        assert_eq!((*string).storage, StringStorage::Unowned);
        assert_eq!((*string).block_room, 0);
        assert_eq!((*string).as_str(), "lit");
    }
}

/// Appending to an immortal literal makes a new string and leaves the
/// literal's text alone.
#[test]
fn test_appending_to_a_literal_makes_a_new_string() {
    let mut literal = LiteralShaped::new("lit", 1usize << 63);
    unsafe {
        let string = literal.string();
        let suffix = make_str("x");
        let joined = miri_rt_string_append(string, suffix);
        assert_ne!(joined, string);
        assert_eq!((*joined).as_str(), "litx");
        assert_eq!((*string).as_str(), "lit");
        miri_rt_string_free(joined);
        miri_rt_string_free(suffix);
    }
}

/// An unowned string nothing else holds is grown by copying its text into a
/// fresh heap buffer, never by reallocating bytes it does not own.
#[test]
fn test_growing_an_unowned_string_copies_its_text() {
    static TEXT: &str = "static";
    let mut literal = LiteralShaped::new(TEXT, 1);
    unsafe {
        let string = literal.string();
        let suffix = make_str("!");
        let grown = miri_rt_string_append(string, suffix);
        assert_eq!(grown, string);
        assert_eq!((*grown).storage, StringStorage::Heap);
        assert_eq!((*grown).as_str(), "static!");
        assert_eq!(TEXT, "static");
        miri_free((*grown).data, (*grown).capacity, 1);
        miri_rt_string_free(suffix);
    }
}

/// Clearing the list a split made releases each piece as a string, whatever
/// bytes the piece holds where a list header keeps its fields.
#[test]
fn test_clearing_a_split_list_releases_its_pieces_as_strings() {
    unsafe {
        let text = make_str("aaaaaaaaaaaaaaaaaaaaaaaa,bbbbbbbbbbbbbbbbbbbbbbbb");
        let separator = make_str(",");
        let list = miri_runtime_core::string::ffi::miri_rt_string_split(text, separator);
        assert_eq!(miri_rt_list_len(list), 2);
        miri_runtime_core::miri_rt_list_clear(list);
        assert_eq!(miri_rt_list_len(list), 0);
        miri_rt_list_free(list);
        miri_rt_string_free(separator);
        miri_rt_string_free(text);
    }
}

/// `index_of_any` of `text` for `chars` from character `start`, freeing both.
unsafe fn index_of_any(text: &str, chars: &str, start: isize) -> isize {
    let haystack = make_str(text);
    let set = make_str(chars);
    let found = miri_rt_string_index_of_any(haystack, set, start);
    miri_rt_string_free(haystack);
    miri_rt_string_free(set);
    found
}

#[test]
fn test_ffi_index_of_any_scans_ascii_text() {
    unsafe {
        assert_eq!(index_of_any("abcde", "a", 0), 0);
        assert_eq!(index_of_any("abcde", "xdc", 0), 2);
        assert_eq!(index_of_any("abcde", "e", 0), 4);
        assert_eq!(index_of_any("abcde", "xyz", 0), -1);
        assert_eq!(index_of_any("a,b,c", ",", 2), 3);
        assert_eq!(index_of_any("a,b,c", ",", 5), -1);
        assert_eq!(index_of_any("a,b,c", ",", 99), -1);
    }
}

#[test]
fn test_ffi_index_of_any_counts_characters_in_multibyte_text() {
    unsafe {
        assert_eq!(index_of_any("héllo, wörld", ",", 0), 5);
        assert_eq!(index_of_any("héllo, wörld", "ö", 0), 8);
        assert_eq!(index_of_any("🦀🦀x🦀y", "xy", 3), 4);
        assert_eq!(index_of_any("日本語のテキスト", "トキ", 5), 5);
        assert_eq!(index_of_any("ééé", "é", 3), -1);
        assert_eq!(index_of_any("abcd", "é🦀", 0), -1);
        assert_eq!(index_of_any("abc🦀d", "🦀", 0), 3);
    }
}

#[test]
fn test_ffi_index_of_any_with_nothing_to_find_or_a_bad_start_is_minus_one() {
    unsafe {
        assert_eq!(index_of_any("abc", "", 0), -1);
        assert_eq!(index_of_any("abc", "a", -1), -1);
        assert_eq!(index_of_any("héllo", "l", isize::MIN), -1);
        let text = make_str("abc");
        assert_eq!(miri_rt_string_index_of_any(text, ptr::null(), 0), -1);
        assert_eq!(miri_rt_string_index_of_any(ptr::null(), text, 0), -1);
        miri_rt_string_free(text);
    }
}

#[test]
fn test_ffi_index_of_any_finds_every_control_character() {
    unsafe {
        let controls: String = (0u8..32).map(char::from).collect();
        for code in 0u8..32 {
            let text = format!("plain é text{}tail", char::from(code));
            assert_eq!(index_of_any(&text, &controls, 0), 12, "code {code}");
        }
        assert_eq!(index_of_any("no controls here", &controls, 0), -1);
    }
}
