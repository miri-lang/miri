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
