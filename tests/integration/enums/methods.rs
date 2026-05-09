// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_enum_method_simple_return() {
    assert_runs_with_output(
        r#"
use system.io

enum Status
    Active
    Inactive

    fn is_active() bool
        return true

fn main()
    let s = Status.Active
    println(f"{s.is_active()}")
"#,
        "true",
    );
}

#[test]
fn test_enum_method_match_on_self() {
    assert_runs_with_output(
        r#"
use system.io

enum Direction
    North
    South

    fn label() String
        match self
            Direction.North: "north"
            Direction.South: "south"

fn main()
    println(Direction.North.label())
    println(Direction.South.label())
"#,
        "north\nsouth",
    );
}

#[test]
fn test_enum_method_with_param() {
    assert_runs_with_output(
        r#"
use system.io

enum Counter
    Value(int)

    fn add(n int) int
        match self
            Counter.Value(v): v + n

fn main()
    let c = Counter.Value(10)
    println(f"{c.add(5)}")
"#,
        "15",
    );
}

#[test]
fn test_must_use_user_defined_enum_discarded() {
    assert_compiler_error(
        r#"
use system.io

must_use enum Token
    Valid(String)
    Invalid

fn tokenize(s String) Token
    Token.Valid(s)

fn main()
    tokenize("hello")
"#,
        "must be used",
    );
}

#[test]
fn test_must_use_user_defined_enum_bound_ok() {
    assert_runs(
        r#"
use system.io

must_use enum Token
    Valid(String)
    Invalid

fn tokenize(s String) Token
    Token.Valid(s)

fn main()
    let t = tokenize("hello")
    match t
        Token.Valid(_): println("ok")
        Token.Invalid: println("bad")
"#,
    );
}

#[test]
fn test_must_use_literal_discarded() {
    assert_compiler_error(
        r#"
must_use enum Coin
    Heads
    Tails

fn main()
    Coin.Heads
"#,
        "must be used",
    );
}
