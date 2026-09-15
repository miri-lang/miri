// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_tuple_alias_no_double_free() {
    assert_runs(
        r#"
let t1 = (1, 2, 3)
let t2 = t1
"#,
    );
}

#[test]
fn test_tuple_reassign() {
    assert_runs(
        r#"
var t = (1, 2, 3)
t = (4, 5, 6)
"#,
    );
}

#[test]
fn test_tuple_with_managed_types() {
    assert_runs(
        r#"
use system.collections.list

// If this crashes, there's a problem with tuple drop code.
// Tuples shouldn't leak memory (verified by leak sanitizer / Miri internal RC checks if any).
let t = (List([1, 2, 3]), "hello")
let l2 = t.0 // Increase RC
let s2 = t.1 // Increase RC
"#,
    );
}

#[test]
fn test_tuple_nested_managed_rc() {
    assert_runs(
        r#"
use system.collections.list
use system.collections.tuple

let t = (List([List([1])]), "outer")
let inner = t.0.element_at(0)
"#,
    );
}

/// Reading a managed element out of a tuple leaves it in the tuple, so a binding
/// that takes it holds a reference of its own and the tuple still releases its
/// element exactly once.
#[test]
fn test_tuple_managed_element_bound_by_let_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let t = (List([1, 2, 3]), f"s{1}")
    let l2 = t.0
    let s2 = t.1
    println(f"{l2.length()} {s2} {t.0.length()} {t.1}")
"#,
        "3 s1 3 s1",
    );
}

/// A managed tuple element passed straight to a function, or to one that
/// returns it, is not released by the call on the tuple's behalf.
#[test]
fn test_tuple_managed_element_passed_to_a_call_is_released_once() {
    assert_heap_guard_output(
        r#"
fn take(x String) String
    return x

fn main()
    var t = (f"a{1}", 1)
    println(t.0)
    let r = take(t.0)
    t = (f"b{2}", 2)
    println(r)
    println(t.0)
"#,
        "a1\na1\nb2",
    );
}
