// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_simple_enum() {
    assert_runs(
        r#"
enum Status
    Ok
    Error

fn main()
    let s = Status.Ok
    "#,
    );
}

#[test]
fn test_payloadless_enum_does_not_leak() {
    // An enum whose variants carry no payload is a value: constructing one in
    // a loop must not accumulate heap allocations.
    assert_heap_guard_ok(
        r#"
enum Status
    Ok
    Error

fn main()
    var i = 0
    while i < 50
        let s = Status.Ok
        i = i + 1
    println("done")
    "#,
    );
}

#[test]
fn test_enum_with_primitive_payload_does_not_leak() {
    // Same for an enum whose payloads are all primitives. An enum carrying a
    // managed payload is released correctly already; this covers the case that
    // falls out of the managed set and so has nothing releasing it.
    assert_heap_guard_ok(
        r#"
enum Flag
    On(bool)
    Off

fn main()
    var i = 0
    while i < 50
        let f = Flag.On(true)
        i = i + 1
    println("done")
    "#,
    );
}

#[test]
fn test_enum_with_data() {
    assert_runs(
        r#"
enum Result
    Success(int)
    Failure(String)

fn main()
    let r = Result.Success(42)
    "#,
    );
}
