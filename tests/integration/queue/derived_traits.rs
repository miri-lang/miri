// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_queue_is_empty() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    if q.is_empty()
        println("empty")
"#,
        "empty",
    );
}

#[test]
fn test_queue_first() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    q.enqueue(10)
    q.enqueue(20)

    let first = q.first()
    match first
        Some(x)
            println(f"first {x}")
        None
            println("no first")
"#,
        "first 10",
    );
}

#[test]
fn test_queue_last() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    q.enqueue(10)
    q.enqueue(20)
    q.enqueue(30)

    let last = q.last()
    match last
        Some(x)
            println(f"last {x}")
        None
            println("no last")
"#,
        "last 30",
    );
}

#[test]
fn test_queue_contains() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    q.enqueue(5)
    q.enqueue(10)
    q.enqueue(15)

    if q.contains(10)
        println("found 10")
    if q.contains(20)
        println("found 20")
    else
        println("no 20")
"#,
        "found 10\nno 20",
    );
}

#[test]
fn test_queue_index_of() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    q.enqueue(10)
    q.enqueue(20)
    q.enqueue(30)

    let idx = q.index_of(20)
    println(f"index {idx}")
"#,
        "index 1",
    );
}

#[test]
fn test_queue_sum() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    q.enqueue(1)
    q.enqueue(2)
    q.enqueue(3)

    let total = q.sum()
    println(f"sum {total ?? 0}")
"#,
        "sum 6",
    );
}

#[test]
fn test_queue_min() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    q.enqueue(5)
    q.enqueue(2)
    q.enqueue(8)

    let minimum = q.min()
    println(f"min {minimum ?? 0}")
"#,
        "min 2",
    );
}

#[test]
fn test_queue_max() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    q.enqueue(5)
    q.enqueue(2)
    q.enqueue(8)

    let maximum = q.max()
    println(f"max {maximum ?? 0}")
"#,
        "max 8",
    );
}

#[test]
fn test_queue_first_on_empty() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    let first = q.first()
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
fn test_queue_sum_on_empty() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    let total = q.sum()
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
fn test_queue_min_on_empty() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    let minimum = q.min()
    match minimum
        None
            println("min none")
        Some(_)
            println("has min")
"#,
        "min none",
    );
}
