// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// Integration tests for Map construction, indexing, methods, and edge cases.

use super::utils::*;

// ==================== Construction ====================

#[test]
fn map_literal_int_values() {
    assert_runs(r#"let m = {"a": 1, "b": 2}"#);
}

#[test]
fn map_literal_single_entry() {
    assert_runs(r#"let m = {"key": 42}"#);
}

#[test]
fn map_literal_string_values() {
    assert_runs(r#"let m = {"name": "Alice", "city": "NYC"}"#);
}

// ==================== Index Read ====================

#[test]
fn map_index_read_int_value() {
    assert_runs_with_output(
        r#"
use system.io

let m = {"a": 1, "b": 2, "c": 3}
let a = m["a"]
let b = m["b"]
let c = m["c"]
println(f"{a}")
println(f"{b}")
println(f"{c}")
"#,
        "1\n2\n3",
    );
}

#[test]
fn map_index_read_single_entry() {
    assert_runs_with_output(
        r#"
use system.io

let m = {"key": 42}
let v = m["key"]
println(f"{v}")
"#,
        "42",
    );
}

// ==================== Index Write ====================

#[test]
fn map_index_write() {
    assert_runs_with_output(
        r#"
use system.io

var m = {"a": 1}
m["a"] = 10
let v = m["a"]
println(f"{v}")
"#,
        "10",
    );
}

#[test]
fn map_index_write_new_key() {
    assert_runs_with_output(
        r#"
use system.io

var m = {"a": 1}
m["b"] = 2
let v = m["b"]
println(f"{v}")
"#,
        "2",
    );
}

// ==================== Length ====================

#[test]
fn map_length() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

let m = {"a": 1, "b": 2, "c": 3}
println(f"{m.length()}")
"#,
        "3",
    );
}

// ==================== Methods ====================

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

// ==================== Edge Cases ====================

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

// ==================== Iteration ====================

#[test]
fn map_for_loop_keys() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

let m = {1: 10, 2: 20, 3: 30}
var sum = 0
for k in m
    sum = sum + k
println(f"{sum}")
"#,
        "6",
    );
}

#[test]
fn map_for_loop_keys_and_values() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

let m = {1: 10, 2: 20, 3: 30}
var key_sum = 0
var val_sum = 0
for k, v in m
    key_sum = key_sum + k
    val_sum = val_sum + v
println(f"{key_sum}")
println(f"{val_sum}")
"#,
        "6\n60",
    );
}

// ==================== Type Errors ====================

#[test]
fn map_wrong_key_type() {
    assert_compiler_error(
        r#"
let m = {"a": 1, "b": 2}
let x = m[42]
"#,
        "Invalid map key type",
    );
}

#[test]
fn map_lowercase_type_not_allowed() {
    assert_compiler_error(
        r#"
fn get(m map<String, int>) int
    return 0
"#,
        "",
    );
}

// ==================== Function Integration ====================

#[test]
fn map_passed_to_function() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn get_value(m Map<String, int>, key String) int
    return m[key]

fn main()
    let m = {"x": 42}
    let v = get_value(m: m, key: "x")
    println(f"{v}")
"#,
        "42",
    );
}
