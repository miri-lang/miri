// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_set_method() {
    assert_runs_with_output(
        r#"
use system.io
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
use system.io
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
use system.io
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
use system.io
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
use system.io
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
use system.io
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
use system.io
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
use system.io
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
use system.io
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
use system.io
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
