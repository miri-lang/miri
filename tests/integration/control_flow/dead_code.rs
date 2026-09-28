// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Statements written after a `return`, `break` or `continue` are unreachable.
//! They must never run, and a later `return` must never replace the value an
//! earlier one already returned.

use super::utils::*;

#[test]
fn test_a_second_return_does_not_override_the_first() {
    assert_runs_with_output(
        r#"
fn f(x int) int
    return x + 10
    return x

fn main()
    println(f"result={f(1)}")
"#,
        "result=11",
    );
}

#[test]
fn test_a_statement_after_return_does_not_run() {
    assert_runs_with_output(
        r#"
fn f()
    println("before")
    return
    println("after")

fn main()
    f()
    println("end")
"#,
        "before\nend",
    );
}

#[test]
fn test_an_assignment_to_the_returned_local_after_return_does_not_change_the_result() {
    assert_runs_with_output(
        r#"
fn g() int
    var r = 5
    return r
    r = 99
    return r

fn main()
    println(f"g={g()}")
"#,
        "g=5",
    );
}

#[test]
fn test_an_if_after_return_does_not_run() {
    assert_runs_with_output(
        r#"
fn h(x int) int
    return x * 2
    if x > 0
        return 100
    return 0

fn main()
    println(f"h={h(3)}")
"#,
        "h=6",
    );
}

#[test]
fn test_a_managed_value_built_after_return_is_neither_returned_nor_leaked() {
    assert_runs_with_output(
        r#"
fn s(x String) String
    return f"{x}-a"
    let dead = f"{x}-b"
    return dead

fn main()
    println(s("one"))
"#,
        "one-a",
    );
}

#[test]
fn test_a_statement_after_break_does_not_run() {
    assert_runs_with_output(
        r#"
fn main()
    var i = 0
    while i < 3
        i += 1
        println("loop")
        break
        println("dead")
    println(f"i={i}")
"#,
        "loop\ni=1",
    );
}

#[test]
fn test_a_statement_after_continue_does_not_run() {
    assert_runs_with_output(
        r#"
fn main()
    var i = 0
    var seen = 0
    while i < 3
        i += 1
        continue
        seen += 1
    println(f"i={i} seen={seen}")
"#,
        "i=3 seen=0",
    );
}
