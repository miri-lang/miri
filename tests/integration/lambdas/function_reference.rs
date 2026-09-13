// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A named function used where a function value is expected.
//!
//! A lambda is a closure struct — `[malloc][RC][fn_ptr][captures...]` — and a
//! call through a function-typed value loads `fn_ptr` from that payload and
//! passes the payload back as an implicit first argument. A bare function name
//! has no such payload, so every one of these spellings used to reach the
//! callee as something that is not a closure.

use super::super::utils::*;

/// A named comparator reaches `sorted_by` and orders a list of structs.
#[test]
fn test_named_function_as_sorted_by_comparator() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct Entry
    word String
    count int

fn ranks_first(a Entry, b Entry) int
    return b.count - a.count

fn main()
    var rows = List<Entry>()
    rows.push(Entry("the", 3))
    rows.push(Entry("fox", 2))
    let top = rows.sorted_by(ranks_first)
    for entry in top
        println(f"{entry.word}:{entry.count}")
    "#,
        "the:3\nfox:2",
    );
}

/// A named function passed to a user-defined higher-order function.
#[test]
fn test_named_function_passed_to_user_function() {
    assert_runs_with_output(
        r#"
fn add(a int, b int) int
    return a + b

fn apply(f fn(a int, b int) int, x int, y int) int
    return f(x, y)

fn main()
    println(f"{apply(add, 2, 3)}")
    "#,
        "5",
    );
}

/// A named function bound to a `let` before the call still carries a callable
/// environment.
#[test]
fn test_named_function_stored_in_let_before_call() {
    assert_runs_with_output(
        r#"
fn triple(x int) int
    return x * 3

fn main()
    let f = triple
    println(f"{f(4)}")
    "#,
        "12",
    );
}

/// The same named function referenced twice produces two working values.
#[test]
fn test_named_function_referenced_twice() {
    assert_runs_with_output(
        r#"
fn negate(x int) int
    return 0 - x

fn apply(f fn(x int) int, n int) int
    return f(n)

fn main()
    println(f"{apply(negate, 7)}")
    println(f"{apply(negate, 9)}")
    "#,
        "-7\n-9",
    );
}

/// A named function reaches `map`.
#[test]
fn test_named_function_as_map_transform() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn doubled(x int) int
    return x * 2

fn main()
    let nums = List([1, 2, 3])
    for n in nums.map(doubled)
        println(f"{n}")
    "#,
        "2\n4\n6",
    );
}

/// A named function reaches `filter`.
#[test]
fn test_named_function_as_filter_predicate() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn is_even(x int) bool
    return x % 2 == 0

fn main()
    let nums = List([1, 2, 3, 4])
    for n in nums.filter(is_even)
        println(f"{n}")
    "#,
        "2\n4",
    );
}

/// A named function reaches `any`.
#[test]
fn test_named_function_as_any_predicate() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn is_big(x int) bool
    return x > 10

fn main()
    let nums = List([1, 2, 30])
    println(f"{nums.any(is_big)}")
    "#,
        "true",
    );
}

/// A named function reaches `reduce`.
#[test]
fn test_named_function_as_reduce_combiner() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn summed(acc int, x int) int
    return acc + x

fn main()
    let nums = List([1, 2, 3, 4])
    println(f"{nums.reduce(0, summed)}")
    "#,
        "10",
    );
}

/// A named function over a managed element type leaves the heap balanced.
#[test]
fn test_named_function_over_managed_elements() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn is_long(word String) bool
    return word.length() > 3

fn main()
    let words = List(["fox", "quick", "brown"])
    for w in words.filter(is_long)
        println(f"{w}")
    "#,
        "quick\nbrown",
    );
}

/// Passing a named function whose signature does not match the parameter is a
/// compile error, not a crash.
#[test]
fn test_named_function_signature_mismatch_is_a_compile_error() {
    assert_compiler_error(
        r#"
fn takes_two(a int, b int) int
    return a + b

fn apply(f fn(x int) int, n int) int
    return f(n)

fn main()
    println(f"{apply(takes_two, 3)}")
    "#,
        "Type mismatch for argument 'f'",
    );
}

/// A named function referenced from inside a lambda body. The thunk is built
/// against the lambda's own lowering context, which is discarded once the
/// lambda body is finished — it has to be carried out to the enclosing function
/// or codegen is left calling a symbol nothing defines.
#[test]
fn test_named_function_referenced_inside_a_lambda_body() {
    assert_runs_with_output(
        r#"
fn base(x int) int
    return x * 2

fn apply(f fn(x int) int, n int) int
    return f(n)

fn main()
    let outer = fn(n int) int: apply(base, n)
    println(f"{outer(5)}")
    "#,
        "10",
    );
}

/// A named function that takes no parameters and one that returns nothing: the
/// thunk's argument count is the closure environment plus however many the
/// function declares, including none.
#[test]
fn test_named_function_with_no_parameters_and_no_return() {
    assert_runs_with_output(
        r#"
fn answer() int
    return 42

fn shout(msg String)
    println(msg)

fn call_it(f fn() int) int
    return f()

fn say_both(f fn(m String), a String, b String)
    f(a)
    f(b)

fn main()
    println(f"{call_it(answer)}")
    say_both(shout, "one", "two")
    "#,
        "42\none\ntwo",
    );
}

/// The comparator snippet leaves the heap balanced: the closure the reference
/// builds is released, and the elements it orders are neither leaked nor freed
/// twice.
#[test]
fn test_named_function_comparator_leaves_the_heap_clean() {
    assert_heap_guard_ok(
        r#"
use system.collections.list

struct Entry
    word String
    count int

fn ranks_first(a Entry, b Entry) int
    return b.count - a.count

fn main()
    var rows = List<Entry>()
    rows.push(Entry("the", 3))
    rows.push(Entry("fox", 2))
    let top = rows.sorted_by(ranks_first)
    for entry in top
        println(f"{entry.word}:{entry.count}")
    "#,
    );
}
