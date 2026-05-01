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

// ── Nested / complex drop scenarios ──────────────────────────────────────────

#[test]
fn test_class_with_nested_class_field_drops_correctly() {
    // Inner class is heap-allocated and reference-counted. Dropping Outer must
    // DecRef Inner; if Inner's RC reaches zero, Inner is freed too.
    assert_runs_with_output(
        r#"
use system.io

class Inner
    var x int

class Outer
    var child Inner

fn make() int
    let o = Outer(child: Inner(x: 99))
    o.child.x

fn main()
    let v = make()
    println(f"{v}")
    "#,
        "99",
    );
}

#[test]
fn test_reassign_class_field_drops_old_value() {
    // Reassigning a class-typed field must DecRef the old object.
    assert_runs_with_output(
        r#"
use system.io

class Node
    var value int

class Holder
    var node Node

fn main()
    var h = Holder(node: Node(value: 1))
    h.node = Node(value: 2)
    println(f"{h.node.value}")
    "#,
        "2",
    );
}

#[test]
fn test_object_shared_between_two_variables_not_freed_early() {
    // Assigning the same object to two variables bumps its RC to 2.
    // Neither variable alone should free it.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let l = List([1, 2, 3])
    let l2 = l
    println(f"{l2.length()}")
    "#,
        "3",
    );
}

#[test]
fn test_drop_in_loop() {
    // Object created inside a loop body must be dropped at end of each iteration,
    // not accumulated until the loop exits.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var i = 0
    while i < 3
        let tmp = List([i])
        i += 1
    println("ok")
    "#,
        "ok",
    );
}

// ── User-defined drop hook (§9.3) ────────────────────────────────────────────

#[test]
fn test_user_drop_hook_called_at_scope_exit() {
    assert_runs_with_output(
        r#"
use system.io

struct Res
    handle int
    fn drop(self)
        println("dropped")

fn main()
    let r = Res(handle: 42)
"#,
        "dropped",
    );
}

#[test]
fn test_user_drop_hook_called_before_parent_returns() {
    assert_runs_with_output(
        r#"
use system.io

struct Token
    id int
    fn drop(self)
        println("token gone")

fn use_token()
    let t = Token(id: 1)
    println("using")

fn main()
    use_token()
    println("after")
"#,
        "using\ntoken gone\nafter",
    );
}

#[test]
fn test_user_drop_hook_multiple_fields_access() {
    // Drop hook can use self fields (via self.x pattern if supported),
    // but here we just verify the hook is called even when struct has multiple fields.
    assert_runs_with_output(
        r#"
use system.io

struct Handle
    fd int
    flags int
    fn drop(self)
        println("handle closed")

fn main()
    let h = Handle(fd: 3, flags: 0)
    println("opened")
"#,
        "opened\nhandle closed",
    );
}

// ── §9.4: Scope-exit warning for unconsumed resources ────────────────────────

#[test]
fn test_scope_exit_warning_emitted_for_unconsumed_resource() {
    assert_compiler_warning(
        r#"
use system.io

struct Conn
    handle int
    fn drop(self)
        return

fn main()
    let conn = Conn(handle: 1)
    println("working")
"#,
        "resource 'conn' of type 'Conn' was not consumed before scope exit",
    );
}

#[test]
fn test_scope_exit_warning_suppressed_when_resource_consumed() {
    // Passing to a consuming function suppresses the warning.
    assert_type_checks(
        r#"
use system.io

struct Conn
    handle int
    fn drop(self)
        return

fn sink(c Conn)
    return

fn main()
    let conn = Conn(handle: 1)
    sink(conn)
"#,
    );
}

#[test]
fn test_scope_exit_warning_in_nested_scope() {
    // Resource declared inside a helper function warns at function exit.
    assert_compiler_warning(
        r#"
use system.io

struct Token
    id int
    fn drop(self)
        return

fn use_token()
    let t = Token(id: 1)
    println("using")

fn main()
    use_token()
"#,
        "resource 't' of type 'Token' was not consumed before scope exit",
    );
}

#[test]
fn test_scope_exit_no_warning_for_non_resource_struct() {
    // Structs without fn drop are not resource types — no warning.
    assert_type_checks(
        r#"
use system.io

struct Point
    x int
    y int

fn main()
    let p = Point(x: 1, y: 2)
    println(f"{p.x}")
"#,
    );
}
