// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_index_write() {
    assert_runs_with_output(
        r#"

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

var m = {"a": 1}
m["b"] = 2
let v = m["b"]
println(f"{v}")
"#,
        "2",
    );
}

/// Writing through an index must take an exclusive copy first, exactly as the
/// `set` method does. Without the check the write lands in the shared buffer
/// and the original binding sees the new entry.
#[test]
fn map_index_write_copies_before_mutating_an_alias() {
    assert_runs_with_output(
        r#"
use system.collections.map

var original = Map<String, int>()
original["x"] = 1
var alias = original
alias["y"] = 2
println(f"{original.length()}")
println(f"{alias.length()}")
"#,
        "1\n2",
    );
}

/// The copy must only happen when the buffer is actually shared — an unaliased
/// map still mutates in place.
#[test]
fn map_index_write_still_mutates_when_unshared() {
    assert_runs_with_output(
        r#"
use system.collections.map

var entries = Map<String, int>()
entries["x"] = 1
entries["y"] = 2
println(f"{entries.length()}")
"#,
        "2",
    );
}

/// A key built only for the write belongs to the map once stored; the
/// temporary that produced it must be released, or it outlives the program.
#[test]
fn map_index_write_releases_a_key_built_for_the_write() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String, int>()
    m["a" + "b"] = 1
    m["a" + "b"] = 2
    let v = m.get("ab") ?? -1
    println(f"{m.length()} {v}")
"#,
        "1 2",
    );
}

/// An optional key written as `Some(..)`, or as a bare value the write wraps
/// into a fresh `Some`, is released the same way.
///
/// Entries are read back by iteration rather than `get`: an optional key is
/// still compared by address, so a lookup misses whatever was stored.
#[test]
fn map_index_write_releases_a_fresh_optional_key() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn main()
    var m = Map<int?, String>()
    m[Some(2)] = "two"
    m[3] = "three"
    var total = 0
    for k, v in m
        total = total + (k ?? 0) * v.length()
    println(f"{m.length()} {total}")
"#,
        "2 21",
    );
}

/// A named key is shared with the map, not handed over: it stays readable after
/// the write, and repeating the write never frees it from under its binding.
#[test]
fn map_index_write_keeps_a_named_key_alive() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String, int>()
    let key = "a" + "b"
    var i = 0
    while i < 50
        m[key] = i
        i = i + 1
    let v = m.get(key) ?? -1
    println(f"{key} {m.length()} {v}")
"#,
        "ab 1 49",
    );
}
