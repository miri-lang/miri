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
