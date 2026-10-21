// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Programs whose loops index a list or an array the loop leaves the header
//! of unchanged, which code generation reads once before the loop. Each pins
//! what the program prints, so reading the header once changes nothing a
//! program can see: copies stay copies, an empty or never-allocated list
//! reads as empty, and an index past the end still stops the program.

use super::utils::*;

const POINT: &str = r#"
use system.io
use system.collections.list

struct Point
    x int
    y int
"#;

fn with_point(main: &str) -> String {
    format!("{POINT}\n{main}")
}

#[test]
fn test_an_alias_taken_before_a_loop_writing_through_the_list_keeps_its_values() {
    let source = with_point(
        r#"
fn main()
    var l = List([Point(x: 1, y: 0), Point(x: 2, y: 0), Point(x: 3, y: 0)])
    let alias = l
    var i = 0
    while i < l.length()
        l[i].x = l[i].x * 10
        i += 1
    println(f"{l[0].x} {l[1].x} {l[2].x} {alias[0].x} {alias[1].x} {alias[2].x}")
"#,
    );
    assert_runs_with_output(&source, "10 20 30 1 2 3\n");
    assert_heap_guard_output(&source, "10 20 30 1 2 3\n");
}

#[test]
fn test_a_list_of_shared_rows_written_in_nested_loops_copies_each_row_it_writes() {
    let source = with_point(
        r#"
fn main()
    let row = List([1, 2, 3])
    var grid = List([row, row])
    var i = 0
    while i < grid.length()
        var line = grid[i]
        var j = 0
        while j < line.length()
            line[j] = line[j] + i + 1
            j += 1
        grid[i] = line
        i += 1
    println(f"{row[0]} {row[2]} {grid[0][0]} {grid[0][2]} {grid[1][0]} {grid[1][2]}")
"#,
    );
    assert_runs_with_output(&source, "1 3 2 4 3 5\n");
    assert_heap_guard_output(&source, "1 3 2 4 3 5\n");
}

#[test]
fn test_a_loop_over_an_empty_and_a_never_allocated_list_reads_no_element() {
    let source = with_point(
        r#"
fn total(l [int]) int
    var i = 0
    var t = 0
    while i < l.length()
        t = t + l[i]
        i += 1
    t

fn main()
    let empty = List<int>()
    var never [int]
    println(f"{total(empty)} {total(never)} {never.length()}")
"#,
    );
    assert_runs_with_output(&source, "0 0 0\n");
}

#[test]
fn test_an_index_past_the_end_written_in_a_loop_still_stops_the_program() {
    let source = with_point(
        r#"
fn main()
    var l = List([1, 2, 3])
    var i = 0
    while i < 5
        l[i] = i
        i += 1
    println(f"{l[0]}")
"#,
    );
    assert_runtime_error(&source, "the len is 3 but the index is 3");
}

#[test]
fn test_an_index_past_the_end_read_in_a_loop_still_stops_the_program() {
    let source = with_point(
        r#"
fn total(l [Point], n int) int
    var i = 0
    var t = 0
    while i < n
        t = t + l[i].x
        i += 1
    t

fn main()
    println(f"{total(List([Point(x: 1, y: 2)]), 2)}")
"#,
    );
    assert_runtime_error(&source, "the len is 1 but the index is 1");
}

#[test]
fn test_a_push_inside_the_loop_is_seen_by_the_next_index() {
    let source = with_point(
        r#"
fn main()
    var l = List([1])
    var i = 0
    while i < 5
        l.push(l[i] * 2)
        i += 1
    println(f"{l.length()} {l[5]}")
"#,
    );
    assert_runs_with_output(&source, "6 32\n");
}

#[test]
fn test_an_array_of_structs_written_in_a_loop_keeps_value_semantics() {
    let source = with_point(
        r#"
fn sum(a [Point; 3]) int
    var i = 0
    var t = 0
    while i < 3
        t = t + a[i].x + a[i].y
        i += 1
    t

fn main()
    var a = [Point(x: 1, y: 1), Point(x: 2, y: 2), Point(x: 3, y: 3)]
    let before = a
    var i = 0
    while i < 3
        a[i].x = a[i].x * 3
        i += 1
    println(f"{sum(a)} {sum(before)} {a[2].x} {before[2].x}")
"#,
    );
    assert_runs_with_output(&source, "24 12 9 3\n");
    assert_heap_guard_output(&source, "24 12 9 3\n");
}

#[test]
fn test_a_pair_loop_over_a_list_of_structs_updates_every_pair() {
    let source = with_point(
        r#"
fn step(start [Point]) [Point]
    var points = start
    let n = points.length()
    var i = 0
    while i < n
        var j = i + 1
        while j < n
            let d = points[i].x - points[j].x
            points[i].y = points[i].y - d
            points[j].y = points[j].y + d
            j += 1
        i += 1
    i = 0
    while i < n
        points[i].x = points[i].x + points[i].y
        i += 1
    points

fn main()
    var points = List([Point(x: 1, y: 0), Point(x: 4, y: 0), Point(x: 9, y: 0)])
    let first = points
    points = step(points)
    println(f"{points[0].x} {points[1].x} {points[2].x} {first[0].x} {first[2].x}")
"#,
    );
    let expected = "12 6 -4 1 9\n";
    assert_runs_with_output(&source, expected);
    assert_heap_guard_output(&source, expected);
}
