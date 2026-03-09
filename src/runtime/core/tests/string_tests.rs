use miri_runtime_core::string::*;
use std::ptr;

// ---------------------------------------------------------------------------
// MiriString inherent methods
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Constructor FFI functions
// ---------------------------------------------------------------------------

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
        let original = Box::into_raw(Box::new(MiriString::from_str("clone me")));
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

// ---------------------------------------------------------------------------
// Inspection FFI functions
// ---------------------------------------------------------------------------

#[test]
fn test_ffi_string_len() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("hello")));
        assert_eq!(miri_rt_string_len(s), 5);
        assert_eq!(miri_rt_string_len(ptr::null()), 0);
        miri_rt_string_free(s);
    }
}

#[test]
fn test_ffi_string_is_empty() {
    unsafe {
        let empty = Box::into_raw(Box::new(MiriString::new()));
        let nonempty = Box::into_raw(Box::new(MiriString::from_str("x")));
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
        let haystack = Box::into_raw(Box::new(MiriString::from_str("Hello, World!")));
        let needle = Box::into_raw(Box::new(MiriString::from_str("World")));
        let missing = Box::into_raw(Box::new(MiriString::from_str("xyz")));

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
        let s = Box::into_raw(Box::new(MiriString::from_str("Hello, World!")));
        let prefix = Box::into_raw(Box::new(MiriString::from_str("Hello")));
        let wrong = Box::into_raw(Box::new(MiriString::from_str("World")));

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
        let s = Box::into_raw(Box::new(MiriString::from_str("Hello, World!")));
        let suffix = Box::into_raw(Box::new(MiriString::from_str("World!")));
        let wrong = Box::into_raw(Box::new(MiriString::from_str("Hello")));

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
        let a = Box::into_raw(Box::new(MiriString::from_str("same")));
        let b = Box::into_raw(Box::new(MiriString::from_str("same")));
        let c = Box::into_raw(Box::new(MiriString::from_str("different")));

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
        let s = Box::into_raw(Box::new(MiriString::from_str("abc")));
        let data = miri_rt_string_data(s);
        assert!(!data.is_null());
        assert_eq!(*data, b'a');
        assert_eq!(miri_rt_string_data(ptr::null()), ptr::null());
        miri_rt_string_free(s);
    }
}

// ---------------------------------------------------------------------------
// Transformation FFI functions
// ---------------------------------------------------------------------------

#[test]
fn test_ffi_concat() {
    unsafe {
        let a = Box::into_raw(Box::new(MiriString::from_str("Hello, ")));
        let b = Box::into_raw(Box::new(MiriString::from_str("World!")));

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
        let s = Box::into_raw(Box::new(MiriString::from_str("hello")));

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
        let s = Box::into_raw(Box::new(MiriString::from_str("HELLO")));
        let result = miri_rt_string_to_lower(s);
        assert_eq!((*result).as_str(), "hello");

        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_to_upper() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("hello")));
        let result = miri_rt_string_to_upper(s);
        assert_eq!((*result).as_str(), "HELLO");

        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_trim() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("  hello  ")));
        let result = miri_rt_string_trim(s);
        assert_eq!((*result).as_str(), "hello");
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_trim_start() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("  hello  ")));
        let result = miri_rt_string_trim_start(s);
        assert_eq!((*result).as_str(), "hello  ");
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_trim_end() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("  hello  ")));
        let result = miri_rt_string_trim_end(s);
        assert_eq!((*result).as_str(), "  hello");
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_replace() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("hello world hello")));
        let from = Box::into_raw(Box::new(MiriString::from_str("hello")));
        let to = Box::into_raw(Box::new(MiriString::from_str("hi")));

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
        let s = Box::into_raw(Box::new(MiriString::from_str("hello")));
        let empty = Box::into_raw(Box::new(MiriString::new()));
        let to = Box::into_raw(Box::new(MiriString::from_str("x")));

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
        let s = Box::into_raw(Box::new(MiriString::from_str("Hello, World!")));
        let sub = miri_rt_string_substring(s, 7, 12);
        assert_eq!((*sub).as_str(), "World");

        // Out of bounds
        let oob = miri_rt_string_substring(s, 5, 100);
        assert!((*oob).is_empty());

        // Reversed indices
        let rev = miri_rt_string_substring(s, 10, 5);
        assert!((*rev).is_empty());

        miri_rt_string_free(s);
        miri_rt_string_free(sub);
        miri_rt_string_free(oob);
        miri_rt_string_free(rev);
    }
}

#[test]
fn test_ffi_substring_unicode_boundary() {
    unsafe {
        // "café" = [99, 97, 102, 195, 169] — 'é' is 2 bytes
        let s = Box::into_raw(Box::new(MiriString::from_str("café")));
        // Trying to slice at byte 4 (middle of 'é') should return empty
        let bad = miri_rt_string_substring(s, 0, 4);
        assert!((*bad).is_empty());

        // Valid slice up to 'f'
        let good = miri_rt_string_substring(s, 0, 3);
        assert_eq!((*good).as_str(), "caf");

        miri_rt_string_free(s);
        miri_rt_string_free(bad);
        miri_rt_string_free(good);
    }
}

#[test]
fn test_ffi_char_at() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("hello")));
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
        let s = Box::into_raw(Box::new(MiriString::from_str("こんにちは")));
        let ch = miri_rt_string_char_at(s, 2);
        assert_eq!((*ch).as_str(), "に");
        miri_rt_string_free(s);
        miri_rt_string_free(ch);
    }
}

#[test]
fn test_ffi_repeat() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("ab")));
        let result = miri_rt_string_repeat(s, 3);
        assert_eq!((*result).as_str(), "ababab");

        let zero = miri_rt_string_repeat(s, 0);
        assert!((*zero).is_empty());

        miri_rt_string_free(s);
        miri_rt_string_free(result);
        miri_rt_string_free(zero);
    }
}

// ---------------------------------------------------------------------------
// Type conversion FFI functions
// ---------------------------------------------------------------------------

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
        let frac = miri_rt_float_to_string(3.14);
        assert_eq!((*frac).as_str(), "3.14");

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

// ---------------------------------------------------------------------------
// Edge cases for conversions
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Additional string edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_ffi_concat_empty_strings() {
    unsafe {
        let a = Box::into_raw(Box::new(MiriString::new()));
        let b = Box::into_raw(Box::new(MiriString::new()));

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
        let a = Box::into_raw(Box::new(MiriString::from_str("Hello ")));
        let b = Box::into_raw(Box::new(MiriString::from_str("世界!")));

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
        let s = Box::into_raw(Box::new(MiriString::from_str("CAFÉ")));
        let result = miri_rt_string_to_lower(s);
        assert_eq!((*result).as_str(), "café");
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_to_upper_unicode() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("café")));
        let result = miri_rt_string_to_upper(s);
        assert_eq!((*result).as_str(), "CAFÉ");
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_trim_tabs_and_newlines() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("\t\n hello \r\n")));
        let result = miri_rt_string_trim(s);
        assert_eq!((*result).as_str(), "hello");
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_trim_all_whitespace() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("   ")));
        let result = miri_rt_string_trim(s);
        assert!((*result).is_empty());
        miri_rt_string_free(s);
        miri_rt_string_free(result);
    }
}

#[test]
fn test_ffi_trim_empty() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::new()));
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
        let from = Box::into_raw(Box::new(MiriString::from_str("x")));
        let to = Box::into_raw(Box::new(MiriString::from_str("y")));
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
        let s = Box::into_raw(Box::new(MiriString::from_str("hello world")));
        let from = Box::into_raw(Box::new(MiriString::from_str("world")));
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
        let s = Box::into_raw(Box::new(MiriString::from_str("hello")));

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
        let s = Box::into_raw(Box::new(MiriString::from_str("abc")));
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
        let haystack = Box::into_raw(Box::new(MiriString::from_str("hello")));
        let empty = Box::into_raw(Box::new(MiriString::new()));

        // In Rust, "hello".contains("") is true
        assert_eq!(miri_rt_string_contains(haystack, empty), 1);

        miri_rt_string_free(haystack);
        miri_rt_string_free(empty);
    }
}

#[test]
fn test_ffi_starts_with_empty() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("hello")));
        let empty = Box::into_raw(Box::new(MiriString::new()));

        // "hello".starts_with("") is true in Rust
        assert_eq!(miri_rt_string_starts_with(s, empty), 1);

        miri_rt_string_free(s);
        miri_rt_string_free(empty);
    }
}

#[test]
fn test_ffi_ends_with_empty() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("hello")));
        let empty = Box::into_raw(Box::new(MiriString::new()));

        assert_eq!(miri_rt_string_ends_with(s, empty), 1);

        miri_rt_string_free(s);
        miri_rt_string_free(empty);
    }
}

#[test]
fn test_ffi_equals_null_vs_empty() {
    unsafe {
        let empty = Box::into_raw(Box::new(MiriString::new()));
        // null treated as empty, so null == empty
        assert_eq!(miri_rt_string_equals(ptr::null(), empty), 1);
        assert_eq!(miri_rt_string_equals(empty, ptr::null()), 1);
        miri_rt_string_free(empty);
    }
}

#[test]
fn test_ffi_string_char_count_ascii() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("hello")));
        assert_eq!(miri_rt_string_char_count(s), 5);
        miri_rt_string_free(s);
    }
}

#[test]
fn test_ffi_string_char_count_empty() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::new()));
        assert_eq!(miri_rt_string_char_count(s), 0);
        assert_eq!(miri_rt_string_char_count(ptr::null()), 0);
        miri_rt_string_free(s);
    }
}

#[test]
fn test_ffi_string_char_count_emoji() {
    unsafe {
        let s = Box::into_raw(Box::new(MiriString::from_str("Hi! 👋🌍")));
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
