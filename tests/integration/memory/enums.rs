// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for memory correctness around enum variants with managed payloads.
// When an enum holds a String or heap-allocated value in a variant, the
// payload must be IncRef'd on construction and DecRef'd when the enum is
// dropped or the variable is reassigned.

use super::super::utils::*;

/// Enum with a String payload: the String must be freed when the enum drops.
#[test]
fn test_enum_string_payload_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

enum Message
    Text(String)
    Empty

fn main()
    let m = Message.Text("hello world")
    let e = Message.Empty
    println("ok")
"#,
        "ok",
    );
}

/// Enum with a List payload: the List must be freed when the enum drops.
#[test]
fn test_enum_list_payload_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

enum Container
    Full(List<int>)
    Empty

fn main()
    let c = Container.Full(List([1, 2, 3]))
    let e = Container.Empty
    println("ok")
"#,
        "ok",
    );
}

/// Enum variable reassigned: the old variant's managed payload must be DecRef'd.
#[test]
fn test_enum_reassignment_drops_old_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

enum Msg
    Text(String)
    Empty

fn main()
    var m = Msg.Text("first")
    m = Msg.Text("second")
    m = Msg.Empty
    println("ok")
"#,
        "ok",
    );
}

/// Enum with managed payload passed to a function; the callee's copy must be
/// freed at function exit without affecting the caller's binding.
#[test]
fn test_enum_passed_to_function_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

enum Status
    Ok(String)
    Fail

fn describe(s Status) String
    match s
        Status.Ok(msg): msg
        Status.Fail: "fail"

fn main()
    let s = Status.Ok("success")
    println(describe(s))
"#,
        "success",
    );
}

/// Match on enum with String payload; the extracted binding is used and freed.
#[test]
fn test_enum_match_string_payload_extraction_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

enum Outcome
    Ok(String)
    Err(String)

fn message(r Outcome) String
    match r
        Outcome.Ok(msg): msg
        Outcome.Err(e): e

fn main()
    let ok = Outcome.Ok("done")
    let err = Outcome.Err("fail")
    println(message(ok))
    println(message(err))
"#,
        "done\nfail",
    );
}

/// Enum with int payload (no managed type) used in match — baseline comparison.
#[test]
fn test_enum_int_payload_match_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

enum Opt
    Some(int)
    None

fn get_or_zero(o Opt) int
    match o
        Opt.Some(n): n
        Opt.None: 0

fn main()
    let a = Opt.Some(42)
    let b = Opt.None
    println(f"{get_or_zero(a)}")
    println(f"{get_or_zero(b)}")
"#,
        "42\n0",
    );
}

/// Multiple enum values with managed payloads created in a loop; each must
/// be freed at iteration end, not accumulated.
#[test]
fn test_enum_with_payload_in_loop_no_accumulation() {
    assert_runs_with_output(
        r#"
use system.io

enum Msg
    Text(String)
    Empty

fn main()
    for i in 0..5
        let m = Msg.Text("item")
    println("ok")
"#,
        "ok",
    );
}

/// Enum holding a managed payload, aliased before being consumed by a function.
#[test]
fn test_enum_alias_then_function_call_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

enum Status
    Active(String)
    Inactive

fn check(s Status) int
    match s
        Status.Active(_): 1
        Status.Inactive: 0

fn main()
    let s1 = Status.Active("running")
    let s2 = s1
    println(f"{check(s1)}")
    println(f"{check(s2)}")
"#,
        "1\n1",
    );
}
