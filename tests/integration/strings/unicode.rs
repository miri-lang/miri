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
