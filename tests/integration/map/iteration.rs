// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_for_loop_custom_type() {
    assert_runs_with_output(
        r#"
use system.collections.map

let items = Map<int, int>()
for i in 0..10
    items.set(i, i * 100)

for k, v in items
    println(f"{k} = {v}")
"#,
        "5 = 500\n4 = 400\n7 = 700\n6 = 600\n1 = 100\n0 = 0\n3 = 300\n2 = 200\n9 = 900\n8 = 800\n",
    );
}

#[test]
fn map_for_loop_keys() {
    assert_runs_with_output(
        r#"
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

#[test]
fn map_iterate_empty_instantiated() {
    assert_runs_with_output(
        r#"
use system.collections.map

let m = Map<int, int>()
var ran = false
for k, v in m
    ran = true

println(f"{ran}")
"#,
        "false",
    );
}

/// Iterating yields the key itself, not a copy, so the loop has to be handed a
/// reference of its own. Without one the loop's release at the end of each pass
/// frees a key the map still holds, and the freed block is handed straight back
/// out to the next allocation — here the string the loop is building.
#[test]
fn test_iterating_heap_keys_does_not_free_them() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn joined(entries {String: int}) String
    var out = "{"
    for key in entries
        out = out + key
    out + "}"

fn build() Map<String, int>?
    var entries = Map<String, int>()
    entries.set("k" + "x", 1)
    Some(entries)

fn rendered() String
    match build()
        Some(entries): joined(entries)
        None: "none"

fn main()
    println(rendered())
"#,
        "{kx}",
    );
}

/// The same guarantee when the map outlives several passes: each pass must
/// leave the key's count exactly where it found it.
#[test]
fn test_repeated_iteration_leaves_keys_intact() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var entries = Map<String, int>()
    var index = 0
    while index < 3
        entries.set(f"key_{index}", index)
        index = index + 1
    var pass = 0
    var total = 0
    while pass < 5
        for key in entries
            total = total + key.length()
        pass = pass + 1
    println(f"{total}")
"#,
        "75",
    );
}

/// A map holding managed values must release them when it is dropped, not only
/// when an entry is overwritten or removed.
#[test]
fn test_dropping_a_map_releases_managed_values() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn stash(count int) int
    var entries = Map<String, String>()
    var index = 0
    while index < count
        entries.set(f"key_{index}", f"value_{index}")
        index = index + 1
    entries.length()

fn main()
    println(f"{stash(20)}")
"#,
        "20",
    );
}

/// Each pass of `for k, v in m` holds its own reference to the value it reads,
/// so the value has to be released before the next pass reads another one.
/// Releasing it only once after the loop frees the last value and leaks the rest.
#[test]
fn test_iterating_heap_values_releases_every_pass() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String, String>()
    m.set("a" + "a", "x" + "y")
    m.set("b" + "b", "z" + "w")
    m.set("c" + "c", "q" + "r")
    var total = 0
    for k, v in m
        total += k.length() + v.length()
    println(f"{total}")
"#,
        "12",
    );
}

#[test]
fn test_iterating_string_to_string_map_parameter_by_size() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn filled(count int) Map<String, String>
    var m = Map<String, String>()
    var i = 0
    while i < count
        m.set(f"key_{i}", f"value_{i}")
        i = i + 1
    m

fn measured(m Map<String, String>) int
    var total = 0
    for k, v in m
        total += k.length() + v.length()
    total

fn main()
    let empty = filled(0)
    let single = filled(1)
    let three = filled(3)
    println(f"{measured(empty)} {measured(single)} {measured(three)}")
"#,
        "0 12 36",
    );
}

#[test]
fn test_iterating_int_to_string_map_parameter_by_size() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn filled(count int) Map<int, String>
    var m = Map<int, String>()
    var i = 0
    while i < count
        m.set(i, f"value_{i}")
        i = i + 1
    m

fn measured(m Map<int, String>) int
    var total = 0
    for k, v in m
        total += k + v.length()
    total

fn main()
    let empty = filled(0)
    let single = filled(1)
    let three = filled(3)
    println(f"{measured(empty)} {measured(single)} {measured(three)}")
"#,
        "0 7 24",
    );
}

#[test]
fn test_iterating_string_to_int_map_parameter_by_size() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn filled(count int) Map<String, int>
    var m = Map<String, int>()
    var i = 0
    while i < count
        m.set(f"key_{i}", i * 10)
        i = i + 1
    m

fn measured(m Map<String, int>) int
    var total = 0
    for k, v in m
        total += k.length() + v
    total

fn main()
    let empty = filled(0)
    let single = filled(1)
    let three = filled(3)
    println(f"{measured(empty)} {measured(single)} {measured(three)}")
"#,
        "0 5 45",
    );
}

/// `break` leaves the loop in the middle of a pass, skipping the release at
/// the end of it; the key and value that pass read are released on the way out
/// instead, exactly once.
#[test]
fn test_break_out_of_map_iteration_releases_the_pass_once() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String, String>()
    m.set("a" + "a", "x" + "y")
    m.set("b" + "b", "z" + "w")
    m.set("c" + "c", "q" + "r")
    var passes = 0
    var total = 0
    for k, v in m
        passes += 1
        total += k.length() + v.length()
        if passes == 2
            break
    println(f"{passes} {total}")
"#,
        "2 8",
    );
}

#[test]
fn test_return_out_of_map_iteration_releases_the_pass() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn value_length_of(m Map<String, String>, wanted String) int
    for k, v in m
        if k == wanted
            return v.length()
    -1

fn main()
    var m = Map<String, String>()
    m.set("a" + "a", "x" + "yz")
    m.set("b" + "b", "z" + "w")
    let bb = "b" + "b"
    let zz = "z" + "z"
    println(f"{value_length_of(m, bb)} {value_length_of(m, zz)}")
"#,
        "2 -1",
    );
}

/// A value copied out of the pass survives the pass's release of its own
/// reference.
#[test]
fn test_value_kept_beyond_its_pass_stays_alive() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String, String>()
    m.set("only" + "", "kept" + "!")
    var last = ""
    for k, v in m
        last = v
    println(last)
"#,
        "kept!",
    );
}
