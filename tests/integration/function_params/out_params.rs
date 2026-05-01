// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// Smoke tests: `out` parameters parse, type-check, lower to MIR, and compile.
// Semantic enforcement of out-write obligations is deferred to a future milestone.

#[test]
fn test_out_param_compiles_and_runs() {
    assert_runs_with_output(
        r#"
use system.io

fn add(a int, b out int) int
    a + b

fn main()
    let r = add(3, 7)
    println(f"{r}")
"#,
        "10",
    );
}

#[test]
fn test_out_param_with_string() {
    assert_runs_with_output(
        r#"
use system.io
use system.string

fn greet(name out String) String
    f"hello {name}"

fn main()
    let r = greet("world")
    println(r)
"#,
        "hello world",
    );
}

#[test]
fn test_multiple_out_params_compile() {
    assert_runs_with_output(
        r#"
use system.io

fn compute(x out int, y out int) int
    x + y

fn main()
    let r = compute(4, 6)
    println(f"{r}")
"#,
        "10",
    );
}
