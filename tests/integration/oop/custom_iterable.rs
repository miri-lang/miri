// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_for_loop_over_class_with_managed_element_type() {
    // The loop variable must be typed as the trait's element type (String), not as
    // the iterable's own type. When it was typed as the class, the element was
    // released through the wrong drop path and its own allocation was never
    // released — a double free plus a leak, which only showed up on some runs.
    assert_runs_with_output(
        r#"
use system.ops

class Words implements Iterable<String>
    var count int

    fn length() int
        return self.count

    fn element_at(index int) String
        return "item"

fn main()
    var words = Words()
    words.count = 2
    for w in words
        println(f"got {w} len {w.length()}")
"#,
        "got item len 4\ngot item len 4",
    );
}

#[test]
fn test_for_loop_over_class_with_scalar_element_type() {
    assert_runs_with_output(
        r#"
use system.ops

class Counter implements Iterable<int>
    var count int

    fn length() int
        return self.count

    fn element_at(index int) int
        return index

fn main()
    var c = Counter()
    c.count = 3
    for n in c
        println(f"value {n + 1}")
"#,
        "value 1\nvalue 2\nvalue 3",
    );
}

/// Iterating a class that yields freshly allocated elements must stay balanced
/// across many iterations. The element count is high because the imbalance this
/// guards against corrupted the heap probabilistically — a two-element loop
/// passed most of the time while the bug was present.
#[test]
fn test_for_loop_over_class_yielding_fresh_allocations_stays_balanced() {
    assert_runs_with_output(
        r#"
use system.ops

class Fresh implements Iterable<String>
    fn length() int
        return 200

    fn element_at(index int) String
        return "a" + "b"

fn main()
    let f = Fresh()
    var seen = 0
    for x in f
        seen += x.length()
    println(f"seen {seen}")
    println("[" + "" + "]")
"#,
        "seen 400\n[]",
    );
}

#[test]
fn test_non_iterable_class_is_rejected() {
    assert_compiler_error(
        r#"
use system.ops

class NonIterable
    var x int

fn main()
    var obj = NonIterable()
    for y in obj
        println(f"{y}")
"#,
        "Type NonIterable is not iterable",
    );
}
