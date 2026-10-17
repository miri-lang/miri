// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A collection's length read straight from its header, and a push stored
//! straight into a list with room for it.
//!
//! Both paths run only while the runtime observes nothing, so these programs
//! run untracked: under the leak check every push would take the runtime call.

use super::utils::{assert_runs_untracked, assert_runs_with_output, get_allocation_count_value};

#[test]
fn test_pushes_past_every_growth_keep_every_element() {
    assert_runs_untracked(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List<int>()
    var i = 0
    while i < 1000
        l.push(i)
        i = i + 1
    var total = 0
    i = 0
    while i < l.length()
        total = total + l[i]
        i = i + 1
    println(f"{l.length()} {total} {l[0]} {l[999]}")
"#,
        "1000 499500 0 999",
    );
}

#[test]
fn test_pushes_of_every_element_width_keep_their_values() {
    assert_runs_untracked(
        r#"
use system.io
use system.collections.list

struct Point
    x int
    y int

fn main()
    var flags = List<bool>()
    var small = List<i16>()
    var words = List<i32>()
    var ratios = List<f32>()
    var reals = List<float>()
    var names = List<String>()
    var points = List<Point>()
    var wide = List<i128>()
    var i = 0
    while i < 20
        flags.push(i % 3 == 0)
        small.push((i - 10) as i16)
        words.push((i * 1000) as i32)
        ratios.push((i as f32) / (4.0 as f32))
        reals.push(i as float / 8.0)
        names.push(f"n{i}")
        points.push(Point(x: i, y: -i))
        wide.push(i as i128 * 1000000000000)
        i = i + 1
    println(f"{flags[3]} {flags[4]} {small[0]} {small[19]} {words[19]} {ratios[2]} {reals[4]} {names[17]} {points[5].y}")
    println(f"{wide[19] == 19000000000000 as i128} {wide.length()}")
"#,
        "true false -10 9 19000 0.5 0.5 n17 -5\ntrue 20",
    );
}

#[test]
fn test_a_push_onto_a_shared_list_leaves_the_other_holder_unchanged() {
    assert_runs_untracked(
        r#"
use system.io
use system.collections.list

fn main()
    var a = List<int>()
    a.push(1)
    let b = a
    a.push(2)
    println(f"{a.length()} {b.length()}")
"#,
        "2 1",
    );
}

#[test]
fn test_the_length_of_every_collection_kind_is_read_from_its_header() {
    assert_runs_untracked(
        r#"
use system.io
use system.collections.list
use system.collections.map
use system.collections.set

fn main()
    let l = List([1, 2, 3])
    let a = [4, 5]
    let s = {1, 2, 3, 4}
    let m = {"a": 1}
    let e = List<int>()
    println(f"{l.length()} {a.length()} {s.length()} {m.length()} {e.length()}")
"#,
        "3 2 4 1 0",
    );
}

#[test]
fn test_pushes_under_the_leak_check_take_the_runtime_path_and_free_everything() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var names = List<String>()
    var i = 0
    while i < 50
        names.push(f"n{i}")
        i = i + 1
    println(f"{names.length()} {names[49]}")
"#,
        "50 n49",
    );
}

#[test]
fn test_the_length_of_a_captured_or_narrowed_list_is_its_own() {
    assert_runs_untracked(
        r#"
use system.io
use system.collections.list

fn count(maybe [int]?) int
    match maybe
        Some(l): return l.length()
        None: return -1

fn main()
    let l = List([1, 2, 3, 4])
    let measure = fn() int: l.length()
    var grown = List([5])
    grown.push(6)
    let shadow = grown
    println(f"{measure()} {count(Some(l))} {count(None)} {shadow.length()}")
"#,
        "4 4 -1 2",
    );
}

/// A list reserved for its final size before it is filled never grows: it
/// allocates its buffer once, where one filled from empty reallocates as it
/// doubles.
#[test]
fn test_a_reserved_list_allocates_its_buffer_once() {
    let fill = |reserve: &str| {
        format!(
            "use system.io\nuse system.collections.list\n\nfn main()\n    var l = List<int>()\n{reserve}    var i = 0\n    while i < 1000\n        l.push(i)\n        i = i + 1\n    println(f\"{{l.length()}}\")\n"
        )
    };
    let reserved = get_allocation_count_value(&fill("    l.reserve(1000)\n"));
    let grown = get_allocation_count_value(&fill(""));
    assert!(
        reserved < grown,
        "reserving allocated {reserved} times, growing {grown}"
    );
}
