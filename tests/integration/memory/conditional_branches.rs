// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for memory correctness in conditional expressions.
// A branch that names a managed local hands the result a copy of it, never the
// local's own reference: only one branch runs, and the local is still released
// at its scope exit on every path, so both branches must reach the join with
// the result owning one reference and the local keeping its own.

use super::super::utils::*;

/// The then-branch reads a managed local; the else-branch builds a fresh array.
#[test]
fn test_conditional_reading_local_on_then_branch_keeps_both_values() {
    assert_heap_guard_output(
        r#"
use system.io

fn pick(c bool) int
    let x = [1, 2]
    let h = if c: x else: [3, 4]
    return h[0]

fn main()
    println(f"{pick(true)}")
    println(f"{pick(false)}")
"#,
        "1\n3",
    );
}

/// The else-branch reads the local, so the move would happen on the other path.
#[test]
fn test_conditional_reading_local_on_else_branch_keeps_both_values() {
    assert_heap_guard_output(
        r#"
use system.io

fn pick(c bool) int
    let x = [1, 2]
    let h = if c: [3, 4] else: x
    return h[0]

fn main()
    println(f"{pick(true)}")
    println(f"{pick(false)}")
"#,
        "3\n1",
    );
}

/// Each branch reads a different local; the one not chosen is still readable.
#[test]
fn test_conditional_reading_a_local_on_each_branch_leaves_both_readable() {
    assert_heap_guard_output(
        r#"
use system.io

fn pick(c bool) int
    let x = [1, 2]
    let y = [7, 8]
    let h = if c: x else: y
    return h[0] * 100 + x[1] * 10 + y[1]

fn main()
    println(f"{pick(true)}")
    println(f"{pick(false)}")
"#,
        "128\n728",
    );
}

/// `unless` swaps the targets; the branch reading the local is the false one.
#[test]
fn test_unless_reading_local_keeps_both_values() {
    assert_heap_guard_output(
        r#"
use system.io

fn pick(c bool) int
    let x = [1, 2]
    let h = unless c: x else: [3, 4]
    return h[0]

fn main()
    println(f"{pick(true)}")
    println(f"{pick(false)}")
"#,
        "3\n1",
    );
}

/// A heap string and a list, read on one branch of a nested conditional.
#[test]
fn test_nested_conditional_reading_string_and_list_locals() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn label(c bool, d bool) String
    let s = "a" + "b"
    let t = if c: (if d: s else: "cd") else: "zz"
    return t

fn count(c bool) int
    let l = List([1, 2, 3])
    let m = if c: l else: List([9])
    return m.length() * 10 + l.length()

fn main()
    println(f"{label(true, true)} {label(true, false)} {label(false, true)}")
    println(f"{count(true)} {count(false)}")
"#,
        "ab cd zz\n33 13",
    );
}

/// A resource read on one branch shares the value; its hook runs exactly once.
#[test]
fn test_conditional_reading_resource_local_runs_hook_once() {
    assert_heap_guard_output(
        r#"
use system.io

class R
    var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"drop {self.id}")

fn pick(c bool)
    let a = R(1)
    let b = if c: a else: R(2)
    println(f"got {b.id}")

fn main()
    pick(true)
    pick(false)
"#,
        "got 1\ndrop 1\ngot 2\ndrop 2\ndrop 1",
    );
}
