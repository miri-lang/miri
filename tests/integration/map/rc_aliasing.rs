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

// ── Map<String, List<int>> scope-exit cleanup (task 2.5) ─────────────────────

#[test]
fn test_map_string_list_values_scope_exit_no_crash() {
    // Map<String, List<int>>: on scope exit both key_drop_fn and val_drop_fn
    // must fire — string keys DecRef'd, list values DecRef'd.
    assert_runs(
        r#"
use system.collections.map
use system.collections.list

fn make()
    let m = {"a": List([1, 2, 3]), "b": List([4, 5])}
    // m goes out of scope — keys and list values must be freed

fn main()
    make()
    make()
"#,
    );
}

#[test]
fn test_map_string_list_values_clear_no_crash() {
    // clear() on Map<String, List<int>> must DecRef both string keys and list
    // values via key_drop_fn / val_drop_fn before the map goes out of scope.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map
use system.collections.list

fn main()
    var m = {"x": List([10, 20]), "y": List([30])}
    m.clear()
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

// ── Map index-write incref for managed elements (task 3.1) ───────────────────

#[test]
fn test_map_index_write_managed_val_incref() {
    // m["k"] = managed_val — after the local val goes out of scope the map must
    // still hold a valid reference (IncRef'd at write time).
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn make_map() Map<String, String>
    let v = "wor" + "ld"
    var m = {"hello": "placeholder"}
    m["hello"] = v
    return m
    // v goes out of scope — map must still own "world" (non-immortal concat string)

fn main()
    let m = make_map()
    println(m["hello"])
"#,
        "world",
    );
}

#[test]
fn test_map_index_write_managed_key_incref() {
    // m[key_var] = 42 — after the local key goes out of scope the map must still
    // hold a valid key pointer (IncRef'd at write time via key_drop_fn).
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn make_map() Map<String, int>
    let k = "my" + "key"
    var m = {"placeholder": 0}
    m[k] = 99
    return m
    // k goes out of scope — map must still have "mykey" as a key (non-immortal)

fn main()
    let m = make_map()
    println(f"{m.length()}")
"#,
        "2",
    );
}
