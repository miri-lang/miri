// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_stack_is_empty() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    if s.is_empty()
        println("empty")
"#,
        "empty",
    );
}

#[test]
fn test_stack_first() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(10)
    s.push(20)

    let first = s.first()
    match first
        Some(x)
            println(f"first {x}")
        None
            println("no first")
"#,
        "first 20",
    );
}

#[test]
fn test_stack_last() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(10)
    s.push(20)
    s.push(30)

    let last = s.last()
    match last
        Some(x)
            println(f"last {x}")
        None
            println("no last")
"#,
        "last 10",
    );
}

#[test]
fn test_stack_contains() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(5)
    s.push(10)
    s.push(15)

    if s.contains(10)
        println("found 10")
    if s.contains(20)
        println("found 20")
    else
        println("no 20")
"#,
        "found 10\nno 20",
    );
}

#[test]
fn test_stack_index_of() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(10)
    s.push(20)
    s.push(30)

    let idx = s.index_of(20)
    println(f"index {idx}")
"#,
        "index 1",
    );
}

#[test]
fn test_stack_sum() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(1)
    s.push(2)
    s.push(3)

    let total = s.sum()
    println(f"sum {total ?? 0}")
"#,
        "sum 6",
    );
}

#[test]
fn test_stack_min() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(5)
    s.push(2)
    s.push(8)

    let minimum = s.min()
    println(f"min {minimum ?? 0}")
"#,
        "min 2",
    );
}

#[test]
fn test_stack_max() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(5)
    s.push(2)
    s.push(8)

    let maximum = s.max()
    println(f"max {maximum ?? 0}")
"#,
        "max 8",
    );
}

#[test]
fn test_stack_first_on_empty() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    let first = s.first()
    match first
        None
            println("first none")
        Some(_)
            println("has first")
"#,
        "first none",
    );
}

#[test]
fn test_stack_sum_on_empty() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    let total = s.sum()
    match total
        None
            println("sum none")
        Some(_)
            println("has sum")
"#,
        "sum none",
    );
}

#[test]
fn test_stack_min_on_empty() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    let minimum = s.min()
    match minimum
        None
            println("min none")
        Some(_)
            println("has min")
"#,
        "min none",
    );
}

#[test]
fn test_stack_reduce_folds_from_the_top() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(4)
    s.push(5)

    let total = s.reduce(0, fn(acc int, x int) int: acc + x)
    println(f"reduce {total}")
"#,
        "reduce 9",
    );
}

#[test]
fn test_stack_count_where_any_and_all() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(1)
    s.push(2)
    s.push(3)

    println(f"count {s.count_where(fn(x int) bool: x > 1)}")
    println(f"any {s.any(fn(x int) bool: x == 2)}")
    println(f"all {s.all(fn(x int) bool: x > 0)}")
"#,
        "count 2\nany true\nall true",
    );
}
