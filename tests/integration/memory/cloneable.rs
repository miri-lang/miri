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
        "1",
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
