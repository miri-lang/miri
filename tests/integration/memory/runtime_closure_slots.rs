// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A runtime function calling a closure lends it every argument, while a
//! closure body owns each resource it is handed. A closure handed to the
//! runtime is wrapped in an adapter that lends its arguments and hands the
//! closure a reference of its own for each one it owns, so every hook runs
//! once, where the value's last owner ends. Every test reads the exact order
//! of the hook's output under the heap guard.

use super::super::utils::*;

const H: &str = r#"
use system.io
use system.collections.list

class H
    var k int
    fn drop(self)
        println(f"closed {self.k}")
"#;

fn with_h(program: &str) -> String {
    format!("{H}\n{program}")
}

/// `sorted_by` over resources: the comparator borrows each element, and each
/// hook runs once, where the last list holding the element ends.
#[test]
fn sorted_by_over_resources_runs_each_hook_once_where_the_lists_end() {
    assert_heap_guard_output(
        &with_h(
            r#"
fn main()
    var xs = List<H>()
    xs.push(H(k: 2))
    xs.push(H(k: 1))
    let s = xs.sorted_by(fn(a H, b H) int: a.k - b.k)
    println(f"len {s.length()} first {s[0].k}")
"#,
        ),
        "len 2 first 1\nclosed 2\nclosed 1",
    );
}

/// A function named as a value, used as the comparator, is adapted the same
/// way.
#[test]
fn sorted_by_with_a_named_comparator_runs_each_hook_once() {
    assert_heap_guard_output(
        &with_h(
            r#"
fn by_key(a H, b H) int
    return a.k - b.k

fn main()
    var xs = List<H>()
    xs.push(H(k: 2))
    xs.push(H(k: 1))
    let s = xs.sorted_by(by_key)
    println(f"len {s.length()} first {s[0].k}")
"#,
        ),
        "len 2 first 1\nclosed 2\nclosed 1",
    );
}

/// The in-place sort handed a lambda directly: the elements stay the
/// list's, and each hook runs once when the list ends.
#[test]
fn an_in_place_sort_over_resources_runs_each_hook_once_where_the_list_ends() {
    assert_heap_guard_output(
        &with_h(
            r#"
@writes("l")
@closure("f, 2, l")
runtime "core" fn miri_rt_list_sort_by(l [H], f fn(H, H) int)

fn main()
    var l = List<H>()
    l.push(H(k: 2))
    l.push(H(k: 1))
    miri_rt_list_sort_by(l, fn(a H, b H) int: a.k - b.k)
    println(f"sorted {l[0].k} {l[1].k}")
"#,
        ),
        "sorted 1 2\nclosed 1\nclosed 2",
    );
}

/// The runtime function named as a value and called through it: the
/// forwarding body is the call the runtime is reached by, and it adapts the
/// comparator the same way.
#[test]
fn a_runtime_sort_named_as_a_value_runs_each_hook_once() {
    assert_heap_guard_output(
        &with_h(
            r#"
@writes("l")
@closure("f, 2, l")
runtime "core" fn miri_rt_list_sort_by(l [H], f fn(H, H) int)

fn main()
    var l = List<H>()
    l.push(H(k: 2))
    l.push(H(k: 1))
    let s = miri_rt_list_sort_by
    s(l, fn(a H, b H) int: a.k - b.k)
    println(f"kept order {l[0].k} {l[1].k}")
"#,
        ),
        "kept order 2 1\nclosed 2\nclosed 1",
    );
}

/// A closure read out of a field is adapted like one built at the call, and
/// the field keeps its own reference.
#[test]
fn a_closure_read_from_a_field_is_adapted_and_released_once() {
    assert_heap_guard_output(
        &with_h(
            r#"
@writes("l")
@closure("f, 2, l")
runtime "core" fn miri_rt_list_sort_by(l [H], f fn(H, H) int)

class By
    var f fn(H, H) int

fn main()
    let by = By(f: fn(a H, b H) int: a.k - b.k)
    var l = List<H>()
    l.push(H(k: 2))
    l.push(H(k: 1))
    miri_rt_list_sort_by(l, by.f)
    println(f"sorted {l[0].k} {l[1].k}")
"#,
        ),
        "sorted 1 2\nclosed 1\nclosed 2",
    );
}
