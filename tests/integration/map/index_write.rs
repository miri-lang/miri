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

#[test]
fn map_index_compound_write_combines_with_the_existing_value() {
    // A compound write is a read-modify-write: the entry has to be read,
    // combined, and stored. Dropping the operator stored the right-hand side on
    // its own, so the old value was silently lost.
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String, int>()
    m["ab"] = 1
    m["ab"] += 4
    let v = m.get("ab") ?? -1
    println(f"{m.length()} {v}")
"#,
        "1 5",
    );
}

#[test]
fn map_index_compound_write_honours_every_operator() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String, int>()
    m["n"] = 20
    m["n"] -= 5
    let a = m.get("n") ?? -1
    println(f"{a}")
    m["n"] *= 3
    let b = m.get("n") ?? -1
    println(f"{b}")
    m["n"] /= 2
    let c = m.get("n") ?? -1
    println(f"{c}")
    m["n"] %= 7
    let d = m.get("n") ?? -1
    println(f"{d}")
"#,
        "15
45
22
1",
    );
}

#[test]
fn map_index_compound_write_of_an_absent_key_reports_it() {
    // The read half of a compound write answers the way a plain `m[k]` read
    // does: a key the map does not hold is an error, not an entry to create.
    assert_runtime_error(
        r#"
use system.collections.map

fn main()
    var m = Map<String, int>()
    m["present"] = 1
    m["absent"] += 4
    println(f"{m.length()}")
"#,
        "map key not found",
    );
}

#[test]
fn map_index_plain_write_of_an_absent_key_still_inserts() {
    // Only the compound form reads first. A plain write creates the entry, and
    // must keep doing so.
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String, int>()
    m["a"] = 1
    m["b"] = 2
    let got = m.get("b") ?? -1
    println(f"{m.length()} {got}")
"#,
        "2 2",
    );
}

#[test]
fn map_index_compound_write_combines_at_the_maps_value_type() {
    // The combined value is typed by the map's value slot, not by the
    // right-hand side, so adding to a float entry keeps the fraction. The result
    // is read back through the index, because reading it through `get` returns a
    // float value's bit pattern converted rather than reinterpreted.
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var f = Map<String, float>()
    f["x"] = 1.5
    f["x"] += 2.25
    let got = f["x"]
    println(f"{got}")
"#,
        "3.75",
    );
}

#[test]
#[ignore = "compound assignment to a String is broken wherever it is written, not only in a map: `var s = \"a\" + \"b\"` then `s += \"c\" + \"d\"` on a plain local leaks a reference and then crashes with SIGBUS. The map write combines with that same operation, so its managed-value case cannot work until the general one does"]
fn map_index_compound_write_concatenates_a_managed_value() {
    // The value read out is a borrow of what the map still holds, and the
    // combined value is a fresh allocation that replaces it, so the entry the
    // write overwrites has to be released exactly once.
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String, String>()
    m["k"] = "a" + "b"
    m["k"] += "c" + "d"
    let got = m.get("k") ?? "none"
    println(f"{got}")
    println(f"{m.length()}")
"#,
        "abcd
1",
    );
}
