// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_match_option_some() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    let result = match input
                    Some(s): f"Some: {s}"
                    None: "None"
    println(result)

test("Hello")
"#,
        "Some: Hello",
    );
}

#[test]
fn test_match_option_none() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    let result = match input
                    Some(s): f"Some: {s}"
                    None: "None"
    println(result)

test(None)
"#,
        "None",
    );
}

#[test]
fn test_if_let_some() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    if let Some(s) = input
        println(f"unwrapped: {s}")

test("Hello")
"#,
        "unwrapped: Hello",
    );
}

#[test]
fn test_if_let_some_none_skips() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    if let Some(s) = input
        println("should not print")
    println("done")

test(None)
"#,
        "done",
    );
}

#[test]
fn test_if_var_some_mutable() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    if var Some(s) = input
        s = f"{s} changed"
        println(s)

test("Hello")
"#,
        "Hello changed",
    );
}

#[test]
fn test_if_var_some_none_skips() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    if var Some(s) = input
        println("should not print")
    println("done")

test(None)
"#,
        "done",
    );
}

#[test]
fn test_while_let_some() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    while let Some(s) = input
        println(f"value: {s}")
        break

test("Hello")
"#,
        "value: Hello",
    );
}

#[test]
fn test_while_let_some_none_skips() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    while let Some(s) = input
        println("should not print")
        break
    println("done")

test(None)
"#,
        "done",
    );
}

#[test]
fn test_while_var_some_mutable() {
    assert_runs_with_output(
        r#"
use system.io

fn test(input String?)
    while var Some(s) = input
        s = f"{s} changed"
        println(s)
        break

test("Hello")
"#,
        "Hello changed",
    );
}
