// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_class_definition() {
    assert_runs(
        r#"
class Counter
    var count int = 0

fn main()
    let c = Counter()
    "#,
    );
}

#[test]
fn test_class_instantiation() {
    assert_runs(
        r#"
class Point
    var x int = 0
    var y int = 0

fn main()
    let p = Point()
    "#,
    );
}

#[test]
fn test_class_with_constructor_args() {
    assert_runs(
        r#"
class Point
    var x int
    var y int

fn main()
    let p = Point(x: 10, y: 20)
    "#,
    );
}

#[test]
fn test_class_field_access() {
    assert_runs_with_output(
        r#"
use system.io

class Point
    var x int
    var y int

fn main()
    let p = Point(x: 10, y: 20)
    println(f"{p.x + p.y}")
    "#,
        "30",
    );
}

// ── Field mutability ──────────────────────────────────────────────────────────

#[test]
fn test_mutable_field_on_mutable_object() {
    // `var p` binds a mutable variable — reassigning fields should compile and run.
    assert_runs_with_output(
        r#"
use system.io

class Point
    var x int
    var y int

fn main()
    var p = Point(x: 1, y: 2)
    p.x = 99
    println(f"{p.x}")
    "#,
        "99",
    );
}

#[test]
fn test_immutable_object_field_mutation_is_error() {
    // `let p` is immutable — mutating its fields must be rejected.
    assert_compiler_error(
        r#"
class Point
    var x int

fn main()
    let p = Point(x: 1)
    p.x = 99
    "#,
        "immutable",
    );
}

#[test]
fn test_class_with_no_fields() {
    // A class with an empty body is legal and instantiable.
    assert_runs(
        r#"
class Empty

fn main()
    let e = Empty()
    "#,
    );
}

#[test]
fn test_class_field_holding_another_class() {
    // Fields whose type is another class should work and keep their RC alive.
    assert_runs_with_output(
        r#"
use system.io

class Inner
    var value int

class Outer
    var inner Inner

fn main()
    let o = Outer(inner: Inner(value: 42))
    println(f"{o.inner.value}")
    "#,
        "42",
    );
}

#[test]
fn test_multiple_instances_independent() {
    // Two instances of the same class must not share field storage.
    assert_runs_with_output(
        r#"
use system.io

class Counter
    var count int

fn main()
    var a = Counter(count: 1)
    var b = Counter(count: 2)
    a.count = 10
    println(f"{a.count}")
    println(f"{b.count}")
    "#,
        "10\n2",
    );
}

#[test]
fn test_deeply_chained_field_access() {
    // Three levels of nested class field access.
    assert_runs_with_output(
        r#"
use system.io

class C
    var val int

class B
    var c C

class A
    var b B

fn main()
    let a = A(b: B(c: C(val: 7)))
    println(f"{a.b.c.val}")
    "#,
        "7",
    );
}
