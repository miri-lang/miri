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
fn test_for_loop_over_class_implementing_a_trait_that_extends_iterable() {
    // The class never names `Iterable`; it names a trait that extends it. The
    // element type is written at that trait's `extends` clause, so it has to be
    // carried down to the class for the loop variable to be typed at all.
    assert_runs_with_output(
        r#"
use system.ops

trait Sized extends Iterable<int>
    fn size_hint() int

class Counter implements Sized
    var count int

    fn size_hint() int
        return self.count

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

#[test]
fn test_for_loop_over_subclass_of_an_iterable_class() {
    // `Sub` names no trait of its own: it reaches `Iterable` through the class
    // it extends, and the loop calls the body that class compiles.
    assert_runs_with_output(
        r#"
use system.ops

class Counter implements Iterable<int>
    var count int

    fn length() int
        return self.count

    fn element_at(index int) int
        return index * 2

class Sub extends Counter

fn main()
    var s = Sub()
    s.count = 3
    for n in s
        println(f"value {n}")
"#,
        "value 0\nvalue 2\nvalue 4",
    );
}

#[test]
fn test_for_loop_over_generic_class_reaching_iterable_through_a_derived_trait() {
    // The element type travels two substitutions: the trait's parameter is
    // bound by the class's `implements` clause, and the class's parameter by
    // the instantiation. A managed element proves both agree — a loop variable
    // left as an opaque parameter releases the element through the wrong drop
    // path, which `assert_runs_with_output` catches as a leak.
    assert_runs_with_output(
        r#"
use system.ops

trait Listable<E> extends Iterable<E>
    fn is_empty() bool

class Bag<T> implements Listable<T>
    fn is_empty() bool
        return false

    fn length() int
        return 200

    fn element_at(index int) T
        return "a" + "b"

fn main()
    let b = Bag<String>()
    var total = 0
    for s in b
        total += s.length()
    println(f"total {total}")
"#,
        "total 400",
    );
}

#[test]
fn test_for_loop_over_subclass_of_a_generic_iterable_class_pins_the_element_type() {
    // The child names no parameter; its `extends` clause pins the parent's, and
    // that is what the element type has to be read at. The elements are built at
    // runtime rather than written as literals, so the loop variable's drop path
    // is really exercised: a literal is reference-count-blind and would let an
    // element type left opaque pass.
    assert_runs_with_output(
        r#"
use system.ops

class Bag<T> implements Iterable<T>
    fn length() int
        return 200

    fn element_at(index int) T
        return "a" + "b"

class Words extends Bag<String>

fn main()
    let w = Words()
    var total = 0
    for s in w
        total += s.length()
    println(f"total {total}")
"#,
        "total 400",
    );
}

#[test]
fn test_for_loop_over_a_subclass_calls_the_length_it_overrides() {
    // `length` and `element_at` are reached independently: the child compiles a
    // body for the one it overrides, and inherits the other from the class that
    // declares it. Reading both off one name would either call a body the child
    // never compiled or ignore the override.
    assert_runs_with_output(
        r#"
use system.ops

class Base implements Iterable<int>
    fn length() int
        return 2

    fn element_at(index int) int
        return index

class Over extends Base
    fn length() int
        return 4

fn main()
    let o = Over()
    for n in o
        println(f"value {n}")
"#,
        "value 0\nvalue 1\nvalue 2\nvalue 3",
    );
}

#[test]
fn test_class_implementing_a_trait_that_does_not_extend_iterable_is_rejected() {
    assert_compiler_error(
        r#"
use system.ops

trait Named
    fn label() String

class Tag implements Named
    fn label() String
        return "tag"

fn main()
    var t = Tag()
    for y in t
        println(f"{y}")
"#,
        "Type Tag is not iterable",
    );
}

#[test]
fn test_traits_that_extend_each_other_are_refused_rather_than_walked_forever() {
    // Deciding iterability means walking `extends` clauses, and a cycle in them
    // must answer rather than spin. The message is the ordinary refusal: this
    // pins that the walk terminates, not that a cycle diagnostic exists.
    assert_compiler_error(
        r#"
use system.ops

trait Ping extends Pong
    fn ping() int

trait Pong extends Ping
    fn pong() int

class Both implements Ping
    fn ping() int
        return 1

    fn pong() int
        return 2

fn main()
    let b = Both()
    for n in b
        println(f"{n}")
"#,
        "Type Both is not iterable",
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
