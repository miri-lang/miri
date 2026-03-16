// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_class_drop_with_string_field() {
    // String field must not corrupt memory when the class is dropped.
    // String literals are immortal (RC not tracked), so this validates
    // the drop path runs without crashing.
    assert_runs_with_output(
        r#"
use system.io

class Person
    var name String
    var age int

fn main()
    let p = Person(name: "Alice", age: 30)
    println(p.name)
    println(f"{p.age}")
    "#,
        "Alice\n30",
    );
}

#[test]
fn test_class_drop_with_list_field() {
    // A List field must be DecRef'd (and freed if RC reaches 0) when the class
    // instance is dropped. Without the fix, the List would leak (RC stays 2).
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Container
    var items [int]
    var count int

fn main()
    let c = Container(items: List([10, 20, 30]), count: 3)
    println(f"{c.count}")
    println(f"{c.items.length()}")
    "#,
        "3\n3",
    );
}

#[test]
fn test_class_drop_with_string_and_list_fields() {
    // Both String and List fields must be handled correctly on drop.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Record
    var label String
    var data [int]

fn main()
    let r = Record(label: "test", data: List([1, 2, 3]))
    println(r.label)
    println(f"{r.data.length()}")
    "#,
        "test\n3",
    );
}

#[test]
fn test_class_drop_in_function_scope() {
    // Class with managed fields created inside a helper function. Fields must
    // be released when the local goes out of scope, not just at program exit.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Record
    var name String
    var data [int]

fn make_and_count() int
    let r = Record(name: "temp", data: List([1, 2, 3, 4, 5]))
    r.data.length()

fn main()
    let n = make_and_count()
    println(f"{n}")
    "#,
        "5",
    );
}

#[test]
fn test_class_drop_multiple_instances() {
    // Multiple class instances with managed fields all going out of scope.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Item
    var name String
    var values [int]

fn main()
    let a = Item(name: "first", values: List([1, 2]))
    let b = Item(name: "second", values: List([3, 4, 5]))
    println(a.name)
    println(f"{b.values.length()}")
    "#,
        "first\n3",
    );
}

#[test]
fn test_class_list_field_reassign() {
    // Reassigning a managed field variable frees the old value.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List([1, 2, 3])
    l = List([4, 5])
    println(f"{l.length()}")
    "#,
        "2",
    );
}
