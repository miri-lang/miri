// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_code_at_ascii() {
    assert_runs_with_output(
        r#"
fn main()
    let s = "hello"
    match s.code_at(0)
        Some(code): println(f"{code}")
        None: println("error")
"#,
        "104",
    );
}

#[test]
fn test_code_at_multibyte() {
    assert_runs_with_output(
        r#"
fn main()
    let s = "café"
    match s.code_at(3)
        Some(code): println(f"{code}")
        None: println("error")
"#,
        "233",
    );
}

#[test]
fn test_code_at_index_zero() {
    assert_runs_with_output(
        r#"
fn main()
    let s = "test"
    match s.code_at(0)
        Some(code): println(f"{code}")
        None: println("none")
"#,
        "116",
    );
}

#[test]
fn test_code_at_last_index() {
    assert_runs_with_output(
        r#"
fn main()
    let s = "test"
    let len = s.length()
    match s.code_at(len - 1)
        Some(code): println(f"{code}")
        None: println("none")
"#,
        "116",
    );
}

#[test]
fn test_code_at_out_of_range() {
    assert_runs_with_output(
        r#"
fn main()
    let s = "hi"
    match s.code_at(10)
        Some(code): println(f"{code}")
        None: println("none")
"#,
        "none",
    );
}

#[test]
fn test_code_at_negative_index() {
    assert_runs_with_output(
        r#"
fn main()
    let s = "hello"
    match s.code_at(-1)
        Some(code): println(f"{code}")
        None: println("none")
"#,
        "none",
    );
}

#[test]
fn test_from_code_point_ascii() {
    assert_runs_with_output(
        r#"
fn main()
    match String.from_code_point(65)
        Some(s): println(s)
        None: println("error")
"#,
        "A",
    );
}

#[test]
fn test_from_code_point_multibyte() {
    assert_runs_with_output(
        r#"
fn main()
    match String.from_code_point(233)
        Some(s): println(s)
        None: println("error")
"#,
        "é",
    );
}

#[test]
fn test_from_code_point_surrogate() {
    assert_runs_with_output(
        r#"
fn main()
    match String.from_code_point(55296)
        Some(s): println("ok")
        None: println("invalid")
"#,
        "invalid",
    );
}

#[test]
fn test_from_code_point_above_max() {
    assert_runs_with_output(
        r#"
fn main()
    match String.from_code_point(1114112)
        Some(s): println("ok")
        None: println("invalid")
"#,
        "invalid",
    );
}

#[test]
fn test_from_code_point_negative() {
    assert_runs_with_output(
        r#"
fn main()
    match String.from_code_point(-1)
        Some(s): println("ok")
        None: println("invalid")
"#,
        "invalid",
    );
}

#[test]
fn test_code_at_roundtrip() {
    assert_runs_with_output(
        r#"
fn main()
    let orig = "café"
    match orig.code_at(3)
        Some(code)
            match String.from_code_point(code)
                Some(ch): println(ch)
                None: println("from_code_point failed")
        None: println("code_at failed")
"#,
        "é",
    );
}

#[test]
fn for_loop_visits_each_character_of_a_mixed_width_string_once() {
    assert_runs_with_output(
        r#"
fn main()
    let s = "añ" + "€😀z"
    var seen = ""
    for ch in s
        seen = seen + ch + "|"
    println(seen)
"#,
        "a|ñ|€|😀|z|",
    );
}

#[test]
fn code_at_reads_out_of_order_after_a_walk() {
    assert_runs_with_output(
        r#"
fn main()
    let s = "añ" + "€😀z"
    var count = 0
    for ch in s
        count = count + 1
    let late = s.code_at(4) ?? -1
    let early = s.code_at(1) ?? -1
    let past = s.code_at(5) ?? -1
    println(f"{count} {late} {early} {past}")
"#,
        "5 122 241 -1",
    );
}

#[test]
fn two_strings_walked_in_step_read_their_own_characters() {
    assert_runs_with_output(
        r#"
fn main()
    let left = "añ" + "€"
    let right = "x" + "yz"
    var pairs = ""
    for i in 0..3
        pairs = pairs + left.element_at(i) + right.element_at(i) + " "
    println(pairs)
"#,
        "ax ñy €z ",
    );
}

/// Each pass builds a new string of the same length and frees the last one, so
/// the allocator is free to hand the new string the old one's address.
#[test]
fn a_new_string_at_a_freed_strings_address_reads_its_own_characters() {
    assert_runs_with_output(
        r#"
fn main()
    var codes = ""
    for i in 0..5
        let s = "é" + f"{i}"
        let count = s.length()
        codes = codes + f"{count}:{s.code_at(1) ?? -1} "
    println(codes)
"#,
        "2:48 2:49 2:50 2:51 2:52 ",
    );
}
