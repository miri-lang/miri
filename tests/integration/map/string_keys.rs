// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! String keys in a map built by `Map<K,V>()` are matched by content.
//!
//! A map literal picks the runtime's key kind from its key operands. A
//! constructor supplies none, so the kind comes from the declared key type
//! instead. Without that the runtime compares the key bytes — the pointer —
//! and two equal strings become two entries that neither overwrite nor find
//! one another.

use super::utils::*;

#[test]
fn constructed_map_treats_equal_string_keys_as_one_entry() {
    assert_runs_with_output(
        r#"
use system.collections.map

var entries = Map<String, int>()
entries.set("a" + "b", 1)
entries.set("a" + "b", 2)
println(f"{entries.length()}")
"#,
        "1",
    );
}

#[test]
fn constructed_map_finds_a_key_equal_to_the_stored_one() {
    assert_runs_with_output(
        r#"
use system.collections.map

var entries = Map<String, int>()
entries.set("a" + "b", 7)
let probe = "a" + "b"
match entries.get(probe)
    Some(v): println(f"{v}")
    None: println("MISS")
"#,
        "7",
    );
}

#[test]
fn constructed_map_overwrites_the_value_under_an_equal_key() {
    assert_runs_with_output(
        r#"
use system.collections.map

var entries = Map<String, int>()
entries.set("k" + "1", 1)
entries.set("k" + "1", 2)
let probe = "k" + "1"
match entries.get(probe)
    Some(v): println(f"{v}")
    None: println("MISS")
"#,
        "2",
    );
}

#[test]
fn constructed_map_keeps_distinct_string_keys_apart() {
    assert_runs_with_output(
        r#"
use system.collections.map

var entries = Map<String, int>()
entries.set("a" + "b", 1)
entries.set("a" + "c", 2)
println(f"{entries.length()}")
"#,
        "2",
    );
}

#[test]
fn constructed_map_removes_by_an_equal_key() {
    assert_runs_with_output(
        r#"
use system.collections.map

var entries = Map<String, int>()
entries.set("a" + "b", 1)
let probe = "a" + "b"
entries.remove(probe)
println(f"{entries.length()}")
"#,
        "0",
    );
}

/// Rewriting one key many times must free each replaced key rather than
/// accumulate them, so the leak check is the assertion that matters here.
#[test]
fn constructed_map_rewriting_one_key_repeatedly_keeps_a_single_entry() {
    assert_runs_with_output(
        r#"
use system.collections.map

var entries = Map<String, int>()
var index = 0
while index < 300
    entries.set("stable" + "key", index)
    index = index + 1
println(f"{entries.length()}")
"#,
        "1",
    );
}

#[test]
fn constructed_map_with_string_keys_does_not_leak_across_many_inserts() {
    assert_runs_with_output(
        r#"
use system.collections.map

var entries = Map<String, int>()
var index = 0
while index < 300
    entries.set(f"key_{index}", index)
    index = index + 1
println(f"{entries.length()}")
"#,
        "300",
    );
}

/// A key held by a named local is released by that local's scope as well as by
/// the map, so the two releases must be balanced by the donation at the call
/// site. Three hundred iterations turn a mismatch into a crash.
#[test]
fn constructed_map_accepts_a_named_local_key_without_double_free() {
    assert_runs_with_output(
        r#"
use system.collections.map

var entries = Map<String, int>()
var index = 0
while index < 300
    let key = f"key_{index}"
    entries.set(key, index)
    index = index + 1
println(f"{entries.length()}")
"#,
        "300",
    );
}

/// The map literal path already registered the key kind, so this is the control
/// proving the defect belonged to the constructor rather than to `set`.
#[test]
fn map_literal_treats_equal_string_keys_as_one_entry() {
    assert_runs_with_output(
        r#"
use system.collections.map

var entries = Map({("a" + "b"): 1})
entries.set("a" + "b", 2)
println(f"{entries.length()}")
"#,
        "1",
    );
}

/// Mutating a map through a second binding must not affect the first.
#[test]
fn constructed_map_set_preserves_value_semantics() {
    assert_runs_with_output(
        r#"
use system.collections.map

var first = Map<String, int>()
first.set("x", 1)
var second = first
second.set("y", 2)
println(f"{first.length()}")
println(f"{second.length()}")
"#,
        "1\n2",
    );
}
