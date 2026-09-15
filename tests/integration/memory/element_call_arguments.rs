// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// A collection element passed straight to a call is read in place: the
// collection still holds it, so the call must neither take the collection's
// reference away nor release a copy it never retained.

use super::super::utils::*;

/// A list element passed to `println`, to a user function, and to a parameter
/// declared optional is released only by the list.
#[test]
fn test_list_element_passed_to_calls_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn show(x String)
    println(x)

fn opt(x String?)
    println(x ?? "none")

fn main()
    var l = List<String>()
    l.push(f"x{1}")
    println(l[0])
    show(l[0])
    opt(l[0])
    println(l[0])
"#,
        "x1\nx1\nx1\nx1",
    );
}

/// An array element passed to a call is released only by the array.
#[test]
fn test_array_element_passed_to_a_call_is_released_once() {
    assert_heap_guard_output(
        r#"
fn main()
    var a = [f"a{1}", f"b{2}"]
    println(a[1])
    println(a[0])
    println(a[1])
"#,
        "b2\na1\nb2",
    );
}

/// A struct element passed by value to a function leaves the list's element
/// intact, managed field and all.
#[test]
fn test_list_struct_element_passed_by_value_stays_intact() {
    assert_heap_guard_output(
        r#"
use system.collections.list

struct Item
    name String
    qty int

fn qty_of(item Item) int
    return item.qty

fn main()
    let items = List([Item(f"bolt{1}", 4), Item("nut", 6)])
    let q = qty_of(items[0])
    println(f"{q} {items[0].qty} {items[0].name}")
"#,
        "4 4 bolt1",
    );
}
