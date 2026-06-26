// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn variable_declaration() {
    assert_runs_many(&[
        "let x = 10",
        "var y = 20",
        "let z int = 30",
        "var w float = 40.0",
        "let s String = \"hello\"",
        "var b bool = true",
    ]);
}

#[test]
fn implicit_typing() {
    assert_runs_with_output(
        r#"
use system.io
let x = 10
println(f"{x}")
        "#,
        "10",
    );
    assert_runs_with_output(
        r#"
use system.io
var x = 20
println(f"{x}")
        "#,
        "20",
    );
}

#[test]
fn out_is_usable_as_identifier() {
    // `out` is reserved only in parameter position (the `name out Type`
    // marker); everywhere else it parses as an ordinary identifier.
    assert_runs_with_output(
        r#"
use system.io

var out = 10
out = out + 5
println(f"{out}")
        "#,
        "15",
    );
    assert_runs_with_output(
        r#"
use system.io

let out = 42
println(f"{out}")
        "#,
        "42",
    );
    // `out` also names a struct field, a named constructor argument, and a
    // member-access property — every position that goes through `identifier()`.
    assert_runs_with_output(
        r#"
use system.io

struct Box
    out int

let b = Box(out: 7)
println(f"{b.out}")
        "#,
        "7",
    );
}

#[test]
fn explicit_typing() {
    assert_runs_with_output(
        r#"
use system.io

let x int = 42
println(f"{x}")
        "#,
        "42",
    );
    assert_runs_with_output(
        r#"
use system.io

var y i64 = 100
println(f"{y}")
        "#,
        "100",
    );
}
