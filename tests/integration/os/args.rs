// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use crate::utils::miri_run_with_args;

#[test]
fn test_args_length_with_no_args() {
    assert_runs_with_output(
        r#"
use system.result
use system.os

fn main()
    let args = Args()
    println(f"count: {args.length()}")
"#,
        "count: 0",
    );
}

#[test]
fn test_args_length_with_args() {
    assert_runs_with_args_and_output(
        r#"
use system.result
use system.os

fn main()
    let args = Args()
    println(f"count: {args.length()}")
"#,
        &["arg1", "arg2", "arg3"],
        "count: 3",
    );
}

#[test]
fn test_args_element_at() {
    assert_runs_with_args_and_output(
        r#"
use system.result
use system.os

fn main()
    let args = Args()
    if args.length() > 0
        println(args.element_at(0))
    if args.length() > 1
        println(args.element_at(1))
    if args.length() > 2
        println(args.element_at(2))
"#,
        &["hello", "world", "foo"],
        "hello\nworld\nfoo",
    );
}

#[test]
fn test_args_iteration() {
    assert_runs_with_args_and_output(
        r#"
use system.result
use system.os

fn main()
    let args = Args()
    for arg in args
        println(arg)
"#,
        &["one", "two"],
        "one\ntwo",
    );
}

#[test]
fn test_args_out_of_bounds_exits() {
    let code = r#"
use system.result
use system.os

fn main()
    let args = Args()
    args.element_at(99)
"#;

    let result = miri_run_with_args(code, &["arg1"]);
    assert!(
        !result.success,
        "Expected out-of-bounds access to exit with error, but got:\n{}",
        result.output()
    );
    assert!(
        result.stderr.contains("out of bounds"),
        "Expected 'out of bounds' message in stderr, got:\n{}",
        result.output()
    );
}
