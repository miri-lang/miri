// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_queue_enqueue_dequeue_fifo_order() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    q.enqueue(1)
    q.enqueue(2)
    q.enqueue(3)

    let first = q.dequeue() ?? 0
    let second = q.dequeue() ?? 0
    let third = q.dequeue() ?? 0

    println(f"{first}")
    println(f"{second}")
    println(f"{third}")
"#,
        "1\n2\n3",
    );
}

#[test]
fn test_queue_dequeue_empty_returns_none() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    let result = q.dequeue()
    match result
        None
            println("empty")
        Some(_)
            println("not empty")
"#,
        "empty",
    );
}

#[test]
fn test_queue_peek_does_not_remove() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    q.enqueue(42)

    let first_peek = q.peek() ?? 0
    let length_after_peek = q.length()
    let second_peek = q.peek() ?? 0

    println(f"first {first_peek}")
    println(f"length {length_after_peek}")
    println(f"second {second_peek}")
"#,
        "first 42\nlength 1\nsecond 42",
    );
}

#[test]
fn test_queue_length_and_is_empty() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    println(f"empty: {q.is_empty()}")
    println(f"length: {q.length()}")

    q.enqueue(1)
    q.enqueue(2)

    println(f"empty: {q.is_empty()}")
    println(f"length: {q.length()}")
"#,
        "empty: true\nlength: 0\nempty: false\nlength: 2",
    );
}

#[test]
fn test_queue_iteration_fifo_order() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    q.enqueue(10)
    q.enqueue(20)
    q.enqueue(30)

    for x in q
        println(f"{x}")
"#,
        "10\n20\n30",
    );
}
