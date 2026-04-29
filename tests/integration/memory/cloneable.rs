// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

// ─────────────────────────────────────────────
// Trait definition visibility
// ─────────────────────────────────────────────

#[test]
fn test_cloneable_trait_recognized() {
    assert_type_checks(
        r#"
use system.memory
"#,
    );
}

// ─────────────────────────────────────────────
// String satisfies Cloneable
// ─────────────────────────────────────────────

#[test]
fn test_string_clone_produces_independent_copy() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory

let s = "hello"
let c = s.clone()
println(c)
"#,
        "hello",
    );
}

#[test]
fn test_string_clone_is_independent() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory

let a = "world"
let b = a.clone()
println(b)
println(a)
"#,
        "world",
    );
}

// ─────────────────────────────────────────────
// List<int> satisfies Cloneable
// ─────────────────────────────────────────────

#[test]
fn test_list_int_clone_produces_copy() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.list

var lst = List<int>([1, 2, 3])
let c = lst.clone()
println(f"{c[0]}")
println(f"{c[1]}")
println(f"{c[2]}")
"#,
        "1\n2\n3",
    );
}

#[test]
fn test_list_clone_is_independent() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.list

var lst = List<int>([10, 20])
let c = lst.clone()
lst.push(30)
println(f"{c.length()}")
"#,
        "2",
    );
}

// ─────────────────────────────────────────────
// Custom struct with primitive fields can call .clone()
// ─────────────────────────────────────────────

#[test]
fn test_struct_with_primitives_implements_cloneable() {
    assert_type_checks(
        r#"
use system.memory

struct Point
    x int
    y int

class CloneablePoint implements Cloneable
    x int
    y int

    fn init(x int, y int)
        self.x = x
        self.y = y

    public fn clone() CloneablePoint
        return CloneablePoint(self.x, self.y)
"#,
    );
}

#[test]
fn test_struct_clone_method_works() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory

class Point implements Cloneable
    x int
    y int

    fn init(x int, y int)
        self.x = x
        self.y = y

    public fn clone() Point
        return Point(self.x, self.y)

let p = Point(3, 4)
let c = p.clone()
println(f"{c.x}")
println(f"{c.y}")
"#,
        "3",
    );
}

// ─────────────────────────────────────────────
// List<String>: exercises elem_drop_fn IncRef path in miri_rt_list_clone
// ─────────────────────────────────────────────

#[test]
fn test_list_string_clone_elements_are_independent() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.list

var lst = List<String>(["hello", "world"])
let c = lst.clone()
println(c[0])
println(c[1])
println(f"{c.length()}")
"#,
        "hello",
    );
}

#[test]
fn test_list_string_clone_no_leak() {
    assert_runs(
        r#"
use system.io
use system.memory
use system.collections.list

var lst = List<String>(["a", "b", "c"])
let c = lst.clone()
println(f"{c.length()}")
"#,
    );
}

#[test]
fn test_empty_list_clone() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.list

var lst = List<int>([])
let c = lst.clone()
println(f"{c.length()}")
"#,
        "0",
    );
}

// ─────────────────────────────────────────────
// Array satisfies Cloneable
// ─────────────────────────────────────────────

#[test]
fn test_array_clone_produces_copy() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.array

let a = [1, 2, 3]
let b = a.clone()
println(f"{b[0]}")
println(f"{b[1]}")
println(f"{b[2]}")
"#,
        "1\n2\n3",
    );
}

#[test]
fn test_array_clone_is_independent() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.array

let a = [10, 20, 30]
let b = a.clone()
println(f"{a.length()}")
println(f"{b.length()}")
"#,
        "3\n3",
    );
}

#[test]
fn test_array_clone_no_leak() {
    assert_runs(
        r#"
use system.io
use system.memory
use system.collections.array

let a = [1, 2, 3]
let b = a.clone()
println(f"{b.length()}")
"#,
    );
}

// ─────────────────────────────────────────────
// Set satisfies Cloneable
// ─────────────────────────────────────────────

#[test]
fn test_set_clone_produces_copy() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.set

var s = {1, 2, 3}
let c = s.clone()
println(f"{c.length()}")
"#,
        "3",
    );
}

#[test]
fn test_set_clone_is_independent() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.set

var s = {10, 20}
let c = s.clone()
s.add(30)
println(f"{c.length()}")
"#,
        "2",
    );
}

#[test]
fn test_set_clone_no_leak() {
    assert_runs(
        r#"
use system.io
use system.memory
use system.collections.set

var s = {1, 2, 3}
let c = s.clone()
println(f"{c.length()}")
"#,
    );
}

// ─────────────────────────────────────────────
// Map satisfies Cloneable
// ─────────────────────────────────────────────

#[test]
fn test_map_clone_produces_copy() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.map

let m = {"a": 1, "b": 2}
let c = m.clone()
println(f"{c.length()}")
"#,
        "2",
    );
}

#[test]
fn test_map_clone_is_independent() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.map

var m = {"x": 10}
let c = m.clone()
m.set("y", 20)
println(f"{c.length()}")
"#,
        "1",
    );
}

#[test]
fn test_map_clone_no_leak() {
    assert_runs(
        r#"
use system.io
use system.memory
use system.collections.map

let m = {"a": 1, "b": 2}
let c = m.clone()
println(f"{c.length()}")
"#,
    );
}

// ─────────────────────────────────────────────
// Set<String>: exercises elem_drop_fn IncRef path in miri_rt_set_clone
// ─────────────────────────────────────────────

#[test]
fn test_set_string_clone_no_leak() {
    assert_runs(
        r#"
use system.io
use system.memory
use system.collections.set

var s = {"alpha", "beta", "gamma"}
let c = s.clone()
println(f"{c.length()}")
"#,
    );
}

#[test]
fn test_set_string_clone_is_independent() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.set

var s = {"hello", "world"}
let c = s.clone()
s.add("extra")
println(f"{c.length()}")
"#,
        "2",
    );
}

// ─────────────────────────────────────────────
// Map<String, String>: exercises key_drop_fn + val_drop_fn IncRef paths
// ─────────────────────────────────────────────

#[test]
fn test_map_string_string_clone_no_leak() {
    assert_runs(
        r#"
use system.io
use system.memory
use system.collections.map

let m = {"a": "alpha", "b": "beta"}
let c = m.clone()
println(f"{c.length()}")
"#,
    );
}

#[test]
fn test_map_string_string_clone_is_independent() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.map

var m = {"x": "one"}
let c = m.clone()
m.set("y", "two")
println(f"{c.length()}")
"#,
        "1",
    );
}

// ─────────────────────────────────────────────
// Type checker: Array, Set, Map satisfy Cloneable
// ─────────────────────────────────────────────

#[test]
fn test_array_implements_cloneable() {
    assert_type_checks(
        r#"
use system.memory
use system.collections.array

let a = [1, 2, 3]
let b = a.clone()
"#,
    );
}

#[test]
fn test_set_implements_cloneable() {
    assert_type_checks(
        r#"
use system.memory
use system.collections.set

var s = {1, 2, 3}
let c = s.clone()
"#,
    );
}

#[test]
fn test_map_implements_cloneable() {
    assert_type_checks(
        r#"
use system.memory
use system.collections.map

let m = {"a": 1}
let c = m.clone()
"#,
    );
}

// ─────────────────────────────────────────────
// Deep clone of Array/List<custom class>: task 6.2a
// ─────────────────────────────────────────────

#[test]
fn test_array_of_custom_objects_clone_is_deep() {
    // After cloning an array of custom objects, mutating an element of the clone
    // must NOT affect the original. Without elem_clone_fn (shallow clone), both
    // arrays share the same Point allocations, and p.x = 99 would corrupt a[0].
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.array

class Point implements Cloneable
    var x int
    var y int

    fn init(x int, y int)
        self.x = x
        self.y = y

    public fn clone() Point
        return Point(self.x, self.y)

let a = [Point(1, 2), Point(3, 4)]
let b = a.clone()
var p = b[0]
p.x = 99
println(f"{a[0].x}")
"#,
        "1",
    );
}

#[test]
fn test_array_of_custom_objects_clone_no_leak() {
    // Cloning an Array<Point> must not leak or double-free: both arrays and all
    // Point objects must be freed when they go out of scope.
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.array

class Point implements Cloneable
    var x int
    var y int

    fn init(x int, y int)
        self.x = x
        self.y = y

    public fn clone() Point
        return Point(self.x, self.y)

fn main()
    let a = [Point(1, 2), Point(3, 4)]
    let b = a.clone()
    println(f"{b[0].x}")
"#,
        "1",
    );
}

#[test]
fn test_list_of_custom_objects_clone_is_deep() {
    // Same independence test for List<Point>.
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.list

class Point implements Cloneable
    var x int
    var y int

    fn init(x int, y int)
        self.x = x
        self.y = y

    public fn clone() Point
        return Point(self.x, self.y)

let lst = List([Point(10, 20), Point(30, 40)])
let copy = lst.clone()
var p = copy[0]
p.x = 99
println(f"{lst[0].x}")
"#,
        "10",
    );
}

// ─────────────────────────────────────────────
// Empty-constructor List<T> / Set<T> clone: task 6.2b
// ─────────────────────────────────────────────

#[test]
fn test_empty_constructor_list_of_custom_objects_clone_is_deep() {
    // List<Point>() takes the empty-constructor path (miri_rt_list_new).
    // elem_clone_fn must be wired so that .clone() deep-copies elements.
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.list

class Point implements Cloneable
    var x int
    var y int

    fn init(x int, y int)
        self.x = x
        self.y = y

    public fn clone() Point
        return Point(self.x, self.y)

var l = List<Point>()
l.push(Point(1, 2))
let m = l.clone()
var p = m[0]
p.x = 99
println(f"{l[0].x}")
"#,
        "1",
    );
}

#[test]
fn test_empty_constructor_list_of_custom_objects_clone_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.list

class Point implements Cloneable
    var x int
    var y int

    fn init(x int, y int)
        self.x = x
        self.y = y

    public fn clone() Point
        return Point(self.x, self.y)

var l = List<Point>()
l.push(Point(3, 4))
let m = l.clone()
println(f"{m[0].x}")
"#,
        "3",
    );
}

// ─────────────────────────────────────────────
// Deep clone of Set<custom class>: task 6.2a/6.2b
// ─────────────────────────────────────────────

#[test]
fn test_set_of_custom_objects_clone_no_leak() {
    // Non-empty {Point(...)} Set literal path. elem_clone_fn is wired in
    // translate_rvalue.rs. Verify no RC leak or double-free on clone + drop.
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.set

class Point implements Cloneable
    var x int
    var y int

    fn init(x int, y int)
        self.x = x
        self.y = y

    public fn clone() Point
        return Point(self.x, self.y)

fn main()
    var s = {Point(7, 8)}
    let c = s.clone()
    println(f"{c.length()}")
"#,
        "1",
    );
}

#[test]
fn test_empty_constructor_set_of_custom_objects_clone_no_leak() {
    // Set<Point>() empty-constructor path. emit_set_clone_fn_for_elem_kind must
    // wire elem_clone_fn so that .clone() deep-copies elements without leaking.
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.set

class Point implements Cloneable
    var x int
    var y int

    fn init(x int, y int)
        self.x = x
        self.y = y

    public fn clone() Point
        return Point(self.x, self.y)

fn main()
    var s = Set<Point>()
    s.add(Point(3, 4))
    let c = s.clone()
    println(f"{c.length()}")
"#,
        "1",
    );
}

// ─────────────────────────────────────────────
// Deep clone of Map<String, custom class>: task 6.2c
// ─────────────────────────────────────────────

#[test]
fn test_map_of_custom_objects_clone_is_deep() {
    // Cloning a Map<String, Point> must deep-copy the values: mutating an entry
    // of the clone must NOT affect the original map.
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.map

class Point implements Cloneable
    var x int
    var y int

    fn init(x int, y int)
        self.x = x
        self.y = y

    public fn clone() Point
        return Point(self.x, self.y)

let m = {"a": Point(1, 2)}
let n = m.clone()
var p = n["a"]
p.x = 99
let orig = m["a"]
println(f"{orig.x}")
"#,
        "1",
    );
}

#[test]
fn test_map_of_custom_objects_clone_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.map

class Point implements Cloneable
    var x int
    var y int

    fn init(x int, y int)
        self.x = x
        self.y = y

    public fn clone() Point
        return Point(self.x, self.y)

fn main()
    let m = {"a": Point(3, 4)}
    let n = m.clone()
    let entry = n["a"]
    println(f"{entry.x}")
"#,
        "3",
    );
}

// ─────────────────────────────────────────────
// 6.3: push to clone does not affect original
// ─────────────────────────────────────────────

#[test]
fn test_list_clone_push_to_clone_does_not_affect_original() {
    // Plan 6.3 exact example: clone a list, push to the clone,
    // original must still have its original length.
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.list

var a = List<int>([1, 2, 3])
var b = a.clone()
b.push(4)
println(f"{a.length()}")
"#,
        "3",
    );
}

// ─────────────────────────────────────────────
// 6.3: struct with managed (List) field — deep copy, independent lifecycle
// ─────────────────────────────────────────────

#[test]
fn test_struct_managed_list_field_clone_is_deep() {
    // Bag.clone() deep-copies its List field. Pushing to the clone's list
    // must NOT change the original's list length.
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.list

class Bag implements Cloneable
    var items [int]

    fn init(items [int])
        self.items = items

    public fn clone() Bag
        return Bag(self.items.clone())

var a = Bag(List([1, 2, 3]))
var b = a.clone()
b.items.push(4)
println(f"{a.items.length()}")
"#,
        "3",
    );
}

#[test]
fn test_struct_managed_list_field_clone_no_leak() {
    // Both the original and the clone must be freed without leaks.
    assert_runs_with_output(
        r#"
use system.io
use system.memory
use system.collections.list

class Bag implements Cloneable
    var items [int]

    fn init(items [int])
        self.items = items

    public fn clone() Bag
        return Bag(self.items.clone())

fn main()
    var a = Bag(List([1, 2]))
    let b = a.clone()
    println(f"{b.items.length()}")
"#,
        "2",
    );
}

#[test]
fn test_struct_string_field_clone_no_double_free() {
    // Clone of a struct with a String field must deep-copy the String so
    // both the original and the clone have independent RC lifecycles.
    assert_runs_with_output(
        r#"
use system.io
use system.memory

class Tagged implements Cloneable
    var tag String

    fn init(tag String)
        self.tag = tag

    public fn clone() Tagged
        return Tagged(self.tag.clone())

fn main()
    let a = Tagged("hello")
    let b = a.clone()
    println(a.tag)
    println(b.tag)
"#,
        "hello\nhello",
    );
}

// ─────────────────────────────────────────────
// Error: missing clone() prevents implementing Cloneable
// ─────────────────────────────────────────────

#[test]
fn test_class_missing_clone_error() {
    assert_compiler_error(
        r#"
use system.memory

class Bad implements Cloneable
    x int
"#,
        "must implement method",
    );
}
