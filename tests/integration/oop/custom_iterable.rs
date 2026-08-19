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

#[test]
fn test_generic_class_implementing_iterable_int_substitution() {
    // A generic class `Container<T> implements Iterable<T>` instantiated as
    // `Container<int>` must have the loop variable typed as `int`, not `Generic("T")`.
    // The loop body does arithmetic (x + 1), which requires x to be int.
    // This proves the element type is correctly substituted at instantiation.
    assert_runs_with_output(
        r#"
use system.collections.list

class Container<T> implements Iterable<T>
    private var items List<T>

    fn init()
        self.items = List<T>()

    fn push(item T)
        self.items.push(item)

    fn length() int
        return self.items.length()

    fn element_at(index int) T
        return self.items.element_at(index)

fn main()
    let c = Container<int>()
    c.push(10)
    c.push(20)
    c.push(30)
    for x in c
        println(f"value {x + 1}")
"#,
        "value 11\nvalue 21\nvalue 31",
    );
}

#[test]
fn test_generic_class_implementing_iterable_string_high_count() {
    // A generic class yielding a managed element type must have both the type
    // checker and MIR agree on the drop path. If the type checker says `String`
    // but MIR sees `Generic("T")`, the element leaks (dropped as a Generic with
    // no allocation), or double-frees (reference count never incremented).
    // This test uses runtime-allocated strings ("a" + "b") to not be RC-blind
    // and truly exercise the drop path; high element count (200) catches probabilistic
    // imbalance. `assert_runs_with_output` fails on a leak, so this proves RC balance.
    assert_runs_with_output(
        r#"
use system.ops

class Queue<T> implements Iterable<T>
    fn length() int
        return 200

    fn element_at(index int) T
        return "a" + "b"

fn main()
    let q = Queue<String>()
    var len_sum = 0
    for s in q
        len_sum += s.length()
    println(f"total {len_sum}")
"#,
        "total 400",
    );
}

#[test]
fn test_generic_class_with_non_first_trait_param() {
    // A class `Pair<K, V> implements Iterable<V>` must correctly map the
    // element type to the second generic parameter, not the first. This proves
    // the substitution uses the trait's parameter, not positional guessing.
    assert_runs_with_output(
        r#"
use system.ops

class Pair<K, V> implements Iterable<V>
    fn length() int
        return 2

    fn element_at(index int) V
        if index == 0
            return "first"
        else
            return "second"

fn main()
    var p = Pair<int, String>()
    var count = 0
    for v in p
        count += v.length()
    println(f"count {count}")
"#,
        "count 11",
    );
}
