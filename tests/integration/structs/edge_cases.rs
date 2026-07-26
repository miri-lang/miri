// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_struct_with_nested_collections() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Complex
    id int
    names [String]

fn main()
    let names = List(["Alice", "Bob"])
    let c = Complex(id: 1, names: names)
    println(c.names[0])
    println(c.names[1])
    c.names.push("Charlie")
    println(f"{c.names.length()}")
    "#,
        "Alice\nBob\n3",
    );
}

#[test]
fn test_struct_equality_comparisons() {
    assert_runs_with_output(
        r#"

struct Point
    x int
    y int

fn main()
    let p1 = Point(x: 1, y: 2)
    let p2 = Point(x: 1, y: 2)
    println(f"{p1 == p2}")
    "#,
        "true",
    );
}

#[test]
fn test_struct_non_drop_method_is_rejected() {
    // Structs are data types: they may define only `drop`. A standalone method
    // is not supported and must be a clear compile error, not an internal
    // compiler error (ICE) at code generation.
    assert_compiler_error(
        r#"
struct P
    v int
    fn get() int
        return self.v

fn main()
    let p = P(v: 42)
"#,
        "cannot define methods",
    );
}

#[test]
fn test_struct_drop_method_still_allowed() {
    // `drop` remains a valid struct method, and defining it leaves the struct's
    // own fields readable.
    assert_runs_with_output(
        r#"
struct Res
    id int
    fn drop(self)
        println("dropped")

fn main()
    let r = Res(id: 1)
    println(f"id={r.id}")
"#,
        "id=1",
    );
}
