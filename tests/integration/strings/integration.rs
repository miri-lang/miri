// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_string_equality() {
    assert_runs_with_output(
        r#"
use system.io

let a = "hello"
let b = "hello"
let c = "world"
if a == b
    println("equal")
if a != c
    println("not equal")
"#,
        "equal\nnot equal", // Original was "equal" but it prints both
    );
}

#[test]
fn test_string_function_parameter() {
    assert_runs_with_output(
        r#"
use system.io

fn greet(name String)
    println(f"Hello, {name}!")

greet("Miri")
"#,
        "Hello, Miri!",
    );
}

#[test]
fn test_string_function_return() {
    assert_runs_with_output(
        r#"
use system.io

fn get_greeting() String
    return "Hello from function"

let s = get_greeting()
println(s)
"#,
        "Hello from function",
    );
}

#[test]
fn test_string_in_conditional() {
    assert_runs_with_output(
        r#"
use system.io

let s = "yes"
if s == "yes"
    println("got yes")
else
    println("got no")
"#,
        "got yes",
    );
}
