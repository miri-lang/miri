// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for memory correctness in match expressions.
// Each match arm may create or alias managed objects; only the taken arm
// executes, so managed locals from non-taken arms must never generate
// phantom DecRef operations, and the taken arm's temporaries must be freed
// promptly when the arm expression is consumed.

use super::super::utils::*;

/// Each arm creates a temporary List; only the taken arm runs.
/// The temporary must be freed immediately after the arm expression completes.
#[test]
fn test_match_temp_list_per_arm_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn label(n int) int
    match n
        1: List([1, 2, 3]).length()
        2: List([10, 20]).length()
        _: List([]).length()

fn main()
    println(f"{label(1)}")
    println(f"{label(2)}")
    println(f"{label(5)}")
"#,
        "3\n2\n0",
    );
}

/// Only the taken arm allocates; non-taken arms must not generate phantom frees.
#[test]
fn test_match_only_taken_arm_allocates_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn maybe_alloc(flag int) int
    match flag
        1: List([1, 2, 3, 4, 5]).length()
        _: 0

fn main()
    println(f"{maybe_alloc(1)}")
    println(f"{maybe_alloc(0)}")
"#,
        "5\n0",
    );
}

/// Outer managed variable referenced in the match result expression must survive.
#[test]
fn test_match_outer_list_used_in_arm_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let items = List([10, 20, 30])
    let n = match items.length()
        3: items.length() + 1
        _: 0
    println(f"{n}")
    println(f"{items.length()}")
"#,
        "4\n3",
    );
}

/// Class instance created as a temporary inside a match arm; must be freed at arm end.
#[test]
fn test_match_class_temp_in_arm_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

class Tag
    var code int

fn classify(n int) int
    match n
        1: Tag(code: 100).code
        2: Tag(code: 200).code
        _: Tag(code: 0).code

fn main()
    println(f"{classify(1)}")
    println(f"{classify(2)}")
    println(f"{classify(5)}")
"#,
        "100\n200\n0",
    );
}

/// Class with a managed List field created inline in a match arm; both the class
/// and the List must be freed when the arm result is consumed.
#[test]
fn test_match_class_with_list_field_in_arm_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Packet
    var data [int]

fn process(flag int) int
    match flag
        1: Packet(data: List([1, 2, 3])).data.length()
        _: Packet(data: List([])).data.length()

fn main()
    println(f"{process(1)}")
    println(f"{process(0)}")
"#,
        "3\n0",
    );
}

/// Match selects one of two outer managed variables; neither must be dropped early.
/// Both must survive until the enclosing scope exits.
#[test]
fn test_match_selects_between_two_lists_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a = List([1, 2])
    let b = List([3, 4, 5])
    let chosen = match 0
        1: a
        _: b
    println(f"{chosen.length()}")
    println(f"{a.length()}")
    println(f"{b.length()}")
"#,
        "3\n2\n3",
    );
}

/// Match inside a loop: per-iteration temporaries must not accumulate.
#[test]
fn test_match_in_loop_no_accumulation() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn score(n int) int
    match n
        0: List([0]).length()
        _: List([n, n * 2]).length()

fn main()
    var total = 0
    for i in 0..5
        total = total + score(i)
    println(f"{total}")
"#,
        "9",
    );
}

/// Match on a bool; the taken arm creates a temporary class, the other does not.
#[test]
fn test_match_on_bool_arm_allocates_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn check(flag bool) int
    match flag
        true: List([1, 2, 3]).length()
        false: 0

fn main()
    println(f"{check(true)}")
    println(f"{check(false)}")
"#,
        "3\n0",
    );
}

/// Match on an int with multiple arms; each arm creates and immediately
/// drops a class instance. Called repeatedly to catch accumulation.
#[test]
fn test_match_multiple_class_arms_called_repeatedly_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

class Node
    var value int

fn pick(n int) int
    match n
        0: Node(value: 0).value
        1: Node(value: 10).value
        2: Node(value: 20).value
        _: Node(value: 99).value

fn main()
    var sum = 0
    for i in 0..4
        sum = sum + pick(i)
    println(f"{sum}")
"#,
        "129",
    );
}
