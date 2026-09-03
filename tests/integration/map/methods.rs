// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_set_method() {
    assert_runs_with_output(
        r#"
use system.collections.map

var m = {"a": 1}
m.set("b", 2)
let v = m["b"]
println(f"{v}")
println(f"{m.length()}")
"#,
        "2\n2",
    );
}

#[test]
fn map_contains_key() {
    assert_runs_with_output(
        r#"
use system.collections.map

let m = {"a": 1, "b": 2}
let has_a = m.contains_key("a")
let has_z = m.contains_key("z")
println(f"{has_a}")
println(f"{has_z}")
"#,
        "true\nfalse",
    );
}

#[test]
fn map_get_method_returns_option() {
    assert_runs_with_output(
        r#"
use system.collections.map

let m = {"a": 42}
let val = m.get("a")
let missing = m.get("b")

let val_str = match val
    Some(v): f"found {v}"
    None: "not found"

let missing_str = match missing
    Some(v): f"found {v}"
    None: "not found"

println(val_str)
println(missing_str)
"#,
        "found 42\nnot found",
    );
}

#[test]
fn map_remove() {
    assert_runs_with_output(
        r#"
use system.collections.map

var m = {"a": 1, "b": 2}
m.remove("a")
println(f"{m.length()}")
let has_a = m.contains_key("a")
println(f"{has_a}")
"#,
        "1\nfalse",
    );
}

#[test]
fn map_clear() {
    assert_runs_with_output(
        r#"
use system.collections.map

var m = {"a": 1, "b": 2}
m.clear()
println(f"{m.length()}")
println(f"{m.is_empty()}")
"#,
        "0\ntrue",
    );
}

#[test]
fn map_is_empty() {
    assert_runs_with_output(
        r#"
use system.collections.map

var m = {"a": 1}
println(f"{m.is_empty()}")
m.clear()
println(f"{m.is_empty()}")
"#,
        "false\ntrue",
    );
}

#[test]
fn map_remove_nonexistent_key() {
    assert_runs_with_output(
        r#"
use system.collections.map

var m = {"a": 1}
m.remove("missing")
println(f"{m.length()}")
"#,
        "1",
    );
}

#[test]
fn map_get_after_remove() {
    assert_runs_with_output(
        r#"
use system.collections.map

var m = {"a": 1}
m.remove("a")
let val = m.get("a")
let missing_str = match val
    Some(v): "found"
    None: "not found"
println(missing_str)
let has_a = m.contains_key("a")
println(f"{has_a}")
"#,
        "not found\nfalse",
    );
}

#[test]
fn map_method_remove_returns_bool() {
    assert_runs_with_output(
        r#"
use system.collections.map

var m = {"a": 1}
let r1 = m.remove("a")
let r2 = m.remove("b")
println(f"{r1}")
println(f"{r2}")
"#,
        "true\nfalse",
    );
}

#[test]
fn map_method_element_and_value_at() {
    assert_runs_with_output(
        r#"
use system.collections.map

let m = {"single": 100}
let k = m.element_at(0)
let v = m.value_at(0)
println(f"{k}")
println(f"{v}")
"#,
        "single\n100",
    );
}

/// Set method with a projected int value field.
#[test]
fn map_set_projected_int_value() {
    assert_runs_with_output(
        r#"
use system.collections.map

struct Entry
    value int

fn main()
    let e = Entry(42)
    var m = Map<String, int>()
    m.set("key", e.value)
    let v = m.get("key")
    match v
        Some(x): println(f"{x}")
        None: println("not found")
"#,
        "42",
    );
}

/// Set method with a projected int key field.
#[test]
fn map_set_projected_int_key() {
    assert_runs_with_output(
        r#"
use system.collections.map

struct Entry
    k int

fn main()
    let e = Entry(1)
    var m = Map<int, String>()
    m.set(e.k, "value")
    let v = m.get(1)
    match v
        Some(x): println(x)
        None: println("not found")
"#,
        "value",
    );
}

/// Index-assign with a projected int value.
#[test]
fn map_index_assign_projected_value() {
    assert_runs_with_output(
        r#"
use system.collections.map

struct Entry
    value int

fn main()
    let e = Entry(99)
    var m = Map<String, int>()
    m["key"] = e.value
    let v = m.get("key")
    match v
        Some(x): println(f"{x}")
        None: println("not found")
"#,
        "99",
    );
}

/// Index-assign with a projected int key.
#[test]
fn map_index_assign_projected_key() {
    assert_runs_with_output(
        r#"
use system.collections.map

struct Entry
    k int

fn main()
    let e = Entry(2)
    var m = Map<int, String>()
    m[e.k] = "hello"
    let v = m.get(2)
    match v
        Some(x): println(x)
        None: println("not found")
"#,
        "hello",
    );
}

/// Index-assign with a projected managed (String) value verifies RC is correct.
/// The source value is used after the assignment to ensure it was not freed.
#[test]
fn map_index_assign_projected_managed_value() {
    assert_runs_with_output(
        r#"
use system.collections.map

struct Item
    name String

fn main()
    let i = Item("widget")
    var m = Map<String, String>()
    m["item"] = i.name
    println(m["item"])
    println(i.name)
"#,
        "widget\nwidget",
    );
}
