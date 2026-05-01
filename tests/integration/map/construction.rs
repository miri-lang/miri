// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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

#[test]
fn map_explicit_constructor_with_literal() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

let m = Map({"a": 1, "b": 2})
let a = m["a"]
let b = m["b"]
println(f"{m.length()}")
println(f"{a}")
println(f"{b}")
"#,
        "2\n1\n2",
    );
}

#[test]
fn map_explicit_constructor_with_literal_string_values() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

let m = Map({"name": "Alice", "city": "NYC"})
println(f"{m.length()}")
println(m["name"])
"#,
        "2\nAlice",
    );
}

#[test]
fn map_explicit_constructor_empty_typed() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

var m = Map<String, int>()
m.set("x", 10)
let x = m["x"]
println(f"{m.length()}")
println(f"{x}")
"#,
        "1\n10",
    );
}
