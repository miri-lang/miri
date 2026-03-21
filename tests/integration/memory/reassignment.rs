// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for reassignment of managed variables and fields.
// Every time a managed place is overwritten, the old value must be DecRef'd.
// If the old RC reaches 0 it must also be deallocated.

use super::super::utils::*;

#[test]
fn test_list_reassigned_many_times_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List([1])
    l = List([2])
    l = List([3])
    l = List([4])
    l = List([5])
    println(f"{l.length()}")
"#,
        "1",
    );
}

#[test]
fn test_map_reassigned_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    var m = {"a": 1}
    m = {"b": 2, "c": 3}
    m = {"d": 4}
    println(f"{m.length()}")
"#,
        "1",
    );
}

#[test]
fn test_class_reassigned_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Payload
    var data [int]

fn main()
    var p = Payload(data: List([1, 2, 3]))
    p = Payload(data: List([10]))
    println(f"{p.data.length()}")
"#,
        "1",
    );
}

#[test]
fn test_set_reassigned_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

fn main()
    var s = {1, 2, 3, 4, 5}
    s = {10, 20}
    println(f"{s.length()}")
"#,
        "2",
    );
}

#[test]
fn test_reassign_while_alias_alive_no_double_free() {
    // After reassigning l1, l2 still holds the old object (RC stayed at 1).
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l1 = List([10, 20, 30])
    let l2 = l1              // RC = 2
    l1 = List([99])          // old RC → 1, new RC = 1
    println(f"{l2.length()}")
    println(f"{l1.length()}")
"#,
        "3\n1",
    );
}

#[test]
fn test_chain_of_aliases_then_reassign_each() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var a = List([1, 2, 3])
    var b = a               // RC(original) = 2
    var c = b               // RC(original) = 3
    a = List([10])          // RC(original) → 2
    b = List([20])          // RC(original) → 1
    // c still holds original
    println(f"{c.length()}")
    println(f"{a.length()}")
    println(f"{b.length()}")
"#,
        "3\n1\n1",
    );
}

#[test]
fn test_class_managed_field_reassigned_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Bucket
    var items [int]

fn main()
    var b = Bucket(items: List([1, 2, 3, 4, 5]))
    b.items = List([99])
    println(f"{b.items.length()}")
"#,
        "1",
    );
}

#[test]
fn test_class_field_reassigned_many_times_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Holder
    var data [int]

fn main()
    var h = Holder(data: List([1]))
    h.data = List([2, 3])
    h.data = List([4, 5, 6])
    h.data = List([7])
    println(f"{h.data.length()}")
"#,
        "1",
    );
}

#[test]
fn test_nested_class_field_reassigned_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

class Inner
    var x int

class Outer
    var inner Inner

fn main()
    var o = Outer(inner: Inner(x: 1))
    o.inner = Inner(x: 2)
    o.inner = Inner(x: 3)
    println(f"{o.inner.x}")
"#,
        "3",
    );
}

#[test]
fn test_conditional_reassignment_both_branches_no_leak() {
    // Both branches assign to the same managed variable; whichever branch runs,
    // the old value must be freed and only the new one survives.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn pick(flag int) int
    var l = List([1, 2, 3])
    if flag == 1
        l = List([10, 20])
    else
        l = List([100])
    l.length()

fn main()
    println(f"{pick(1)}")
    println(f"{pick(0)}")
"#,
        "2\n1",
    );
}

#[test]
fn test_reassign_to_alias_of_self_no_double_free() {
    // x = x: IncRef before DecRef of old, net change zero, no premature free.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var x = List([1, 2, 3])
    let y = x   // RC = 2
    x = y       // old DecRef (RC→1), new IncRef (RC→2) — or equivalent
    println(f"{x.length()}")
"#,
        "3",
    );
}
