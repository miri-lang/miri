// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_overwrite_existing_key() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

var m = {"a": 1}
m["a"] = 99
let v = m["a"]
println(f"{v}")
println(f"{m.length()}")
"#,
        "99\n1",
    );
}

#[test]
fn map_many_entries() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

let m = {"a": 1, "b": 2, "c": 3, "d": 4, "e": 5}
let a = m["a"]
let b = m["b"]
let c = m["c"]
let d = m["d"]
let e = m["e"]
println(f"{a} {b} {c} {d} {e}")
println(f"{m.length()}")
"#,
        "1 2 3 4 5\n5",
    );
}

#[test]
fn map_int_keys() {
    assert_runs_with_output(
        r#"
use system.io

let m = {1: "one", 2: "two", 3: "three"}
println(m[1])
println(m[2])
println(m[3])
"#,
        "one\ntwo\nthree",
    );
}

#[test]
fn map_clear_and_reuse() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

var m = {"a": 1, "b": 2}
m.clear()
m["c"] = 3
m["d"] = 4
println(f"{m.length()}")
let c_val = m["c"]
println(f"{c_val}")
"#,
        "2\n3",
    );
}

#[test]
fn map_large_scale_operations() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

let m = Map<int, int>()
for i in 0..150
    m.set(i, i * 2)

println(f"{m.length()}")

for i in 0..150
    m.remove(i)

println(f"{m.length()}")
"#,
        "150\n0",
    );
}

#[test]
fn map_nested_maps_rc() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn make_nested() Map<String, Map<String, int>>
    let inner = {"a": 1}
    let outer = {"b": inner}
    return outer

fn main()
    let nested = make_nested()
    let inner = nested["b"]
    let val = inner["a"]
    println(f"{val}")
"#,
        "1",
    );
}

#[test]
fn map_super_deeply_nested() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

let m = {"a": {"b": {"c": {"d": 42}}}}
let v1 = m["a"]["b"]["c"]["d"]
println(f"{v1}")
"#,
        "42",
    );
}

#[test]
fn map_complex_struct_objects() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

struct User
    age int

fn main()
    let alice = User(age: 30)
    let m = {"alice": alice}
    let retrieved = m["alice"]
    println(f"{retrieved.age}")
"#,
        "30",
    );
}

#[test]
fn map_array_as_value() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map
use system.collections.array

let arr = [1, 2, 3]
let m = {"nums": arr}
let retrieved = m["nums"]
let val = retrieved[1]
println(f"{val}")
"#,
        "2",
    );
}

#[test]
fn map_complex_object_keys() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

let p = (10, 20)
let m = {p: "here"}
let val = m[p]
println(val)
"#,
        "here",
    );
}
