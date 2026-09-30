// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A lambda written without a return type returns what its body's last
//! expression gives it, unless that expression is an assignment.

use super::super::utils::*;

#[test]
fn an_unannotated_lambda_returns_its_last_expression() {
    assert_runs_with_output(
        r#"
fn main()
    let x = 3
    let f = fn()
        x + 1
    println(f"{f()}")
"#,
        "4",
    );
}

#[test]
fn an_unannotated_lambda_ending_in_an_assignment_returns_nothing() {
    assert_runs_with_output(
        r#"
class Counter
    public var n int
    fn init()
        self.n = 0

fn main()
    var c = Counter()
    let set = fn()
        c.n = 5
    set()
    println(f"{c.n}")
"#,
        "5",
    );
}

#[test]
fn an_unannotated_lambda_returning_a_managed_value_releases_it() {
    assert_heap_guard_output(
        r#"
fn main()
    let s = "a" + "b"
    let f = fn()
        s + "x"
    println(f())
"#,
        "abx",
    );
}

/// A closure writes a field of an object it captured, through the capture,
/// whether the store is its last statement or not.
#[test]
fn a_closure_writes_through_a_captured_object() {
    assert_heap_guard_output(
        r#"
class Inner
    public var name String
    fn init()
        self.name = "a" + ""

class Holder
    public var n int
    public var label String
    public var inner Inner
    fn init()
        self.n = 0
        self.label = "x" + ""
        self.inner = Inner()

fn stored(b int) int
    var h = Holder()
    let f = fn()
        h.n = b
    f()
    return h.n

fn main()
    var t = Holder()
    let g = fn()
        t.label = "y" + "z"
        t.inner.name = "q" + "r"
        println("set")
    g()
    println(f"{stored(7)} {t.label} {t.inner.name}")
"#,
        "set\n7 yz qr",
    );
}
