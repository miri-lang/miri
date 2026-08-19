// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_stack_push_pop_lifo_order() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(1)
    s.push(2)
    s.push(3)

    let first = s.pop() ?? 0
    let second = s.pop() ?? 0
    let third = s.pop() ?? 0

    println(f"{first}")
    println(f"{second}")
    println(f"{third}")
"#,
        "3\n2\n1",
    );
}

#[test]
fn test_stack_pop_empty_returns_none() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    let result = s.pop()
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
fn test_stack_peek_does_not_remove() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(42)

    let first_peek = s.peek() ?? 0
    let length_after_peek = s.length()
    let second_peek = s.peek() ?? 0

    println(f"first {first_peek}")
    println(f"length {length_after_peek}")
    println(f"second {second_peek}")
"#,
        "first 42\nlength 1\nsecond 42",
    );
}

#[test]
fn test_stack_length_and_is_empty() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    println(f"empty: {s.is_empty()}")
    println(f"length: {s.length()}")

    s.push(1)
    s.push(2)

    println(f"empty: {s.is_empty()}")
    println(f"length: {s.length()}")
"#,
        "empty: true\nlength: 0\nempty: false\nlength: 2",
    );
}

#[test]
fn test_stack_iteration_lifo_order() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(10)
    s.push(20)
    s.push(30)

    for x in s
        println(f"{x}")
"#,
        "30\n20\n10",
    );
}

#[test]
fn test_stack_element_at_flips_index() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(10)
    s.push(20)
    s.push(30)

    println(f"at 0: {s.element_at(0)}")
    println(f"at 1: {s.element_at(1)}")
    println(f"at 2: {s.element_at(2)}")
"#,
        "at 0: 30\nat 1: 20\nat 2: 10",
    );
}

#[test]
fn test_stack_first_is_top() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(10)
    s.push(20)
    s.push(30)

    let top = s.first()
    let next_pop = s.peek()

    match (top, next_pop)
        (Some(t), Some(np))
            if t == np
                println("first equals peek")
"#,
        "first equals peek",
    );
}

#[test]
fn test_stack_last_is_bottom() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(10)
    s.push(20)
    s.push(30)

    let bottom = s.last() ?? 0
    println(f"bottom {bottom}")
"#,
        "bottom 10",
    );
}
