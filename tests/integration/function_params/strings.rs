// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_string_identity_param() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn identity_str(s String) String
    s

fn main()
    let r = identity_str("hello")
    println(r)
"#,
        "hello",
    );
}

#[test]
fn test_string_param_size() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn str_len(s String) int
    s.size()

fn main()
    let r = str_len("hello")
    println(f"{r}")
"#,
        "5",
    );
}

#[test]
fn test_string_two_params_concat() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn join(a String, b String) String
    a.concat(b)

fn main()
    let r = join("hello, ", "world!")
    println(r)
"#,
        "hello, world!",
    );
}

#[test]
fn test_string_param_equality_check() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn is_empty_str(s String) bool
    s.is_empty()

fn main()
    let r = if is_empty_str("")
        1
    else
        0
    println(f"{r}")
"#,
        "1",
    );
}
