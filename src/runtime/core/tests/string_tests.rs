use miri_runtime_core::string::*;

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
fn test_ffi_contains() {
    unsafe {
        let haystack = Box::into_raw(Box::new(MiriString::from_str("Hello, World!")));
        let needle = Box::into_raw(Box::new(MiriString::from_str("World")));

        assert_eq!(miri_rt_string_contains(haystack, needle), 1);

        miri_rt_string_free(haystack);
        miri_rt_string_free(needle);
    }
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
