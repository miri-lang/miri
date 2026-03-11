// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_fstring_method_call() {
    assert_runs_with_output(
        r#"
use system.io

fn double(x int) int
    return x * 2

print(f"double(5) = {double(5)}")
"#,
        "double(5) = 10",
    );
}

#[test]
fn test_fstring_string_method_call() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

let s = "hello"
print(f"{s.to_upper()}")
"#,
        "HELLO",
    );
}

#[test]
fn test_fstring_in_function_return() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn greet(name String) String
    f"Hello, {name}!"

print(greet("Miri"))
"#,
        "Hello, Miri!",
    );
}

#[test]
fn test_fstring_in_function_body_with_int() {
    assert_runs_with_output(
        r#"
use system.io

fn describe(n int) String
    f"value={n}"

print(describe(7))
"#,
        "value=7",
    );
}

#[test]
fn test_fstring_as_println_argument() {
    assert_runs_with_output(
        r#"
use system.io

let n = 42
println(f"answer={n}")
"#,
        "answer=42",
    );
}

#[test]
fn test_fstring_as_string_param() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn show(s String)
    println(s)

let x = 99
show(f"x is {x}")
"#,
        "x is 99",
    );
}
