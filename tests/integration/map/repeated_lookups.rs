// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A map remembers where it last found a key, so a write right after a read
//! of the same key — `m[k] = (m.get(k) ?? 0) + 1` — does not search again.
//! These programs check that what it remembers never answers for another key.

use super::utils::{assert_runs_untracked, assert_runs_with_output};

#[test]
fn test_counting_with_interleaved_keys_counts_each_key() {
    assert_runs_untracked(
        r#"
use system.io
use system.collections.map

fn main()
    var counts = Map<String, int>()
    let words = ["a", "b", "a", "c", "b", "a", "d", "a"]
    for w in words
        counts[w] = (counts.get(w) ?? 0) + 1
    println(f"{counts['a']} {counts['b']} {counts['c']} {counts['d']} {counts.length()}")
"#,
        "4 2 1 1 4",
    );
}

#[test]
fn test_temporary_string_keys_are_told_apart() {
    // Each key is a string built for the lookup and freed after it, so the
    // next one may be allocated where it was, at the same length.
    assert_runs_untracked(
        r#"
use system.io
use system.collections.map

fn main()
    var m = Map<String, int>()
    var i = 0
    while i < 200
        m[f"k{i % 7}"] = (m.get(f"k{i % 7}") ?? 0) + i
        let probe = m.get(f"k{(i + 3) % 7}") ?? -1
        if probe < -1
            println("unreachable")
        i = i + 1
    var total = 0
    var k = 0
    while k < 7
        total = total + (m.get(f"k{k}") ?? 0)
        k = k + 1
    println(f"{m.length()} {total} {m['k0']} {m['k6']}")
"#,
        "7 19900 2842 2814",
    );
}

#[test]
fn test_a_removed_key_is_not_found_where_it_was() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn main()
    var m = {1: 10, 2: 20, 3: 30}
    let before = m.get(2) ?? 0
    m.remove(2)
    let after = m.get(2) ?? -1
    m[2] = 22
    m[4] = (m.get(4) ?? 0) + 4
    println(f"{before} {after} {m[2]} {m[4]} {m.length()}")
"#,
        "20 -1 22 4 4",
    );
}
