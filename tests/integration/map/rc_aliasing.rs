// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ── Drop-fn setter wiring (task 2.4) ─────────────────────────────────────────

#[test]
fn test_map_string_values_remove_no_crash() {
    // Map<String, String>: val_drop_fn must be registered so that remove()
    // properly DecRefs the string value. Construct with a literal so the
    // element type is visible to codegen at construction time.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    var m = {"a": "hello", "b": "world"}
    m.remove("a")
    println(f"{m.length()}")
"#,
        "1",
    );
}

#[test]
fn test_map_string_values_clear_no_crash() {
    // Map<String, String>: clear() must DecRef all string values via val_drop_fn.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    var m = {"x": "alpha", "y": "beta"}
    m.clear()
    println(f"{m.length()}")
"#,
        "0",
    );
}

#[test]
fn test_map_string_key_variable_no_crash() {
    // Map<String, int> with a variable key: key_drop_fn should fire on remove.
    // Previously only string *constant* keys were detected; now Copy/Move locals work too.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    let key = "foo"
    var m = {key: 42}
    m.remove("foo")
    println(f"{m.length()}")
"#,
        "0",
    );
}

#[test]
fn test_map_alias_no_double_free() {
    assert_runs(
        r#"
use system.collections.map
let m1 = {"a": 1, "b": 2}
let m2 = m1 // IncRef
// Both out of scope, shouldn't crash
"#,
    );
}

#[test]
fn test_map_reassign_frees_old() {
    assert_runs(
        r#"
use system.collections.map
var m = {"a": 1}
m = {"b": 2} // frees old
"#,
    );
}

#[test]
fn test_map_passed_to_function_no_dangle() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn consume(m Map<String, int>)
    // goes out of scope

fn main()
    let m = {"k": 99}
    consume(m)
    println(f"{m.length()}")
"#,
        "1",
    );
}
