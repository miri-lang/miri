// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The runtime half of `pieces` and `lines`: counting the pieces of a string,
//! finding where each ends, and building one piece from its byte range.

use miri_runtime_core::string::*;

unsafe fn make_str(s: &str) -> *mut MiriString {
    miri_rt_string_from_raw(s.as_ptr(), s.len())
}

/// Every piece of `text`, walked the way the stdlib walks them: from the end
/// of one piece past the separator to the end of the next.
unsafe fn walk(text: &str, sep: &str) -> Vec<String> {
    let s = make_str(text);
    let separator = make_str(sep);
    let count = miri_rt_string_piece_count(s, separator);
    let mut start = 0;
    let mut pieces = Vec::new();
    for _ in 0..count {
        let end = miri_rt_string_piece_end(s, separator, start);
        let piece = miri_rt_string_slice_bytes(s, start, end);
        pieces.push((*piece).as_str().to_string());
        miri_rt_string_free(piece);
        start = end + sep.len() as i64;
    }
    miri_rt_string_free(separator);
    miri_rt_string_free(s);
    pieces
}

#[test]
fn test_walked_pieces_match_split() {
    let cases = [
        ("", ","),
        ("abc", ","),
        ("a,b,", ","),
        (",a,,b", ","),
        ("a::b::::c", "::"),
        ("héllo wörld", " "),
        ("aébéc", "é"),
    ];
    for (text, sep) in cases {
        let expected: Vec<String> = text.split(sep).map(str::to_string).collect();
        assert_eq!(unsafe { walk(text, sep) }, expected, "{text:?} by {sep:?}");
    }
}

#[test]
fn test_an_empty_separator_walks_characters() {
    assert_eq!(unsafe { walk("héy", "") }, ["h", "é", "y"]);
    assert!(unsafe { walk("", "") }.is_empty());
}

#[test]
fn test_line_count_ignores_one_trailing_newline() {
    let cases = [
        ("", 0),
        ("a", 1),
        ("a\n", 1),
        ("a\nb", 2),
        ("\n", 1),
        ("\n\n", 2),
        ("a\r\nb\r\n", 2),
    ];
    for (text, lines) in cases {
        unsafe {
            let s = make_str(text);
            assert_eq!(miri_rt_string_line_count(s), lines, "{text:?}");
            miri_rt_string_free(s);
        }
    }
}

#[test]
fn test_line_text_drops_one_carriage_return_only_before_a_newline() {
    unsafe {
        let s = make_str("a\r\nb\r\r\n");
        let first = miri_rt_string_line_text(s, 0, 2);
        let second = miri_rt_string_line_text(s, 3, 6);
        assert_eq!((*first).as_str(), "a");
        assert_eq!((*second).as_str(), "b\r");
        miri_rt_string_free(first);
        miri_rt_string_free(second);
        miri_rt_string_free(s);
        let unterminated = make_str("a\r");
        let last = miri_rt_string_line_text(unterminated, 0, 2);
        assert_eq!((*last).as_str(), "a\r");
        miri_rt_string_free(last);
        miri_rt_string_free(unterminated);
    }
}

#[test]
fn test_a_range_off_a_character_boundary_or_the_string_is_clamped() {
    unsafe {
        let s = make_str("é");
        let past = miri_rt_string_slice_bytes(s, 0, 9);
        let inside = miri_rt_string_slice_bytes(s, 1, 2);
        assert_eq!((*past).as_str(), "é");
        assert_eq!((*inside).as_str(), "");
        assert_eq!(miri_rt_string_piece_end(s, s, 1), 2);
        miri_rt_string_free(past);
        miri_rt_string_free(inside);
        miri_rt_string_free(s);
    }
}

#[test]
fn test_an_index_in_range_is_handed_back() {
    assert_eq!(miri_rt_string_piece_index(0, 1), 0);
    assert_eq!(miri_rt_string_piece_index(4, 5), 4);
}
