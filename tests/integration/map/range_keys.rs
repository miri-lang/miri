// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A map keyed by ranges is indexed by its key: a range in `m[...]` names an
//! entry, it does not slice the map.

use super::utils::*;

#[test]
fn a_map_keyed_by_ranges_is_written_and_read_by_index() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let r = 0..3
    var m = Map<Range<int>, int>()
    m[r] = 2
    m[0..3] = 5
    m[1..=4] = 7
    println(f"{m.length()} {m[r]} {m[0..3]} {m[1..=4]}")
"#,
        "2 5 5 7",
    );
}

#[test]
fn a_map_keyed_by_ranges_takes_compound_assignment_by_index() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<Range<int>, int>()
    m[0..3] = 1
    m[0..3] += 10
    let v = m[0..3]
    println(f"{v}")
"#,
        "11",
    );
}

#[test]
fn a_map_keyed_by_ranges_of_floats_is_indexed_by_its_key() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<Range<float>, String>()
    m[0.5..1.5] = "a"
    let v = m[0.5..1.5]
    println(v)
"#,
        "a",
    );
}

#[test]
fn a_range_indexing_a_map_with_other_keys_is_a_key_type_error() {
    assert_compiler_error(
        r#"
let m = {"a": 1, "b": 2}
let x = m[0..1]
"#,
        "Invalid map key type",
    );
}

#[test]
fn a_missing_range_key_is_reported_like_any_missing_key() {
    assert_runtime_error(
        r#"
use system.collections.map

fn main()
    var m = Map<Range<int>, int>()
    m[0..1] = 1
    let v = m[5..6]
    println(f"{v}")
"#,
        "map key not found",
    );
}

#[test]
fn a_map_keyed_by_string_ranges_matches_keys_by_content() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<Range<String>, int>()
    let a = "a"
    m[a.."z"] = 1
    m["a".."z"] += 4
    var k = "b".."c"
    m[k] = 9
    k.end = "d"
    m[k] = 3
    let x = m["a".."z"]
    let y = m[k]
    let z = m["b".."c"]
    println(f"{m.length()} {x} {y} {z}")
"#,
        "3 5 3 9",
    );
}
