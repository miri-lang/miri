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

// ── Phase 10: Copy-on-Write value semantics ───────────────────────────────────

#[test]
fn test_map_cow_set_isolates_original() {
    // CoW: m2 shares m1's data until m2.set mutates → m1 must be unchanged.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    let m1 = {"a": 1, "b": 2}
    var m2 = m1
    m2.set("c", 3)
    println(f"{m1.length()}")
    let has_c = m1.contains_key("c")
    println(f"{has_c}")
"#,
        "2\nfalse",
    );
}

#[test]
fn test_map_cow_remove_isolates_original() {
    // remove triggers CoW — original key must remain.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    let m1 = {"a": 1, "b": 2}
    var m2 = m1
    m2.remove("a")
    println(f"{m1.length()}")
    let has_a = m1.contains_key("a")
    println(f"{has_a}")
"#,
        "2\ntrue",
    );
}

#[test]
fn test_map_cow_clear_isolates_original() {
    // clear triggers CoW — original must be unaffected.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    let m1 = {"a": 1, "b": 2}
    var m2 = m1
    m2.clear()
    println(f"{m1.length()}")
"#,
        "2",
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

// ── Map index-write overwrite decref (task 3.2) ──────────────────────────────

#[test]
fn test_map_index_write_overwrite_managed_no_leak() {
    // m[k] = new_val when key already exists must DecRef old managed value via
    // val_drop_fn. Overwriting 100 times with concat strings verifies no leak.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    var m = {"k": "seed"}
    var i = 0
    while i < 100
        m["k"] = "x" + "y"
        i = i + 1
    println(m["k"])
"#,
        "xy",
    );
}

// ── Nested collection val_drop_fn ────────────────────────────────────────────

#[test]
fn test_map_of_arrays_remove_no_leak() {
    // Map<String, Array<int>>: val_drop_fn must be miri_rt_array_decref_element
    // so that remove() properly DecRefs the inner array.
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var m = {"x": [1, 2, 3]}
    var i = 0
    while i < 50
        m["x"] = [4, 5, 6]
        i = i + 1
    let v = m["x"]
    println(f"{v[0]}")
"#,
        "4",
    );
}

#[test]
fn test_map_of_maps_remove_no_leak() {
    // Map<String, Map<String,int>>: val_drop_fn must be miri_rt_map_decref_element
    // so that overwriting a key properly DecRefs the old inner map.
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var m = {"x": {"a": 1}}
    var i = 0
    while i < 50
        m["x"] = {"b": 2}
        i = i + 1
    let inner = m["x"]
    let val = inner["b"]
    println(f"{val}")
"#,
        "2",
    );
}

// ── Task 3.3: Clear decref all elements ─────────────────────────────────────

#[test]
fn test_map_100_string_keys_clear_no_leak() {
    // Map<String, int>: insert entries with non-immortal (concatenated) string keys,
    // then call clear(). MIRI_LEAK_CHECK=1 catches any key not DecRef'd by key_drop_fn.
    // Runs 100 iterations alternating two distinct non-immortal keys to ensure both
    // are DecRef'd on each clear() cycle.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    var total = 0
    var i = 0
    while i < 100
        var m = {"a" + "a": 1, "b" + "b": 2, "c" + "c": 3, "d" + "d": 4, "e" + "e": 5}
        total = total + m.length()
        m.clear()
        i = i + 1
    println(f"{total}")
"#,
        "500",
    );
}
