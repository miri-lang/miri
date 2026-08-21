// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Queue<T> at non-int element widths.
//!
//! The backing list stores an element at the width its type argument declares,
//! so a float goes in and comes back out as the same bits. Reading it back at
//! the pointer-width fallback instead is what these guard against.

use crate::integration::utils::*;

#[test]
fn test_queue_float_enqueue_dequeue() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    var q = Queue<float>()
    q.enqueue(1.5)
    q.enqueue(2.5)
    q.enqueue(3.5)
    println(f"{q.dequeue() ?? 0.0}")
    println(f"{q.dequeue() ?? 0.0}")
    println(f"{q.dequeue() ?? 0.0}")
"#,
        "1.5
2.5
3.5",
    );
}

#[test]
fn test_queue_float_mixed_operations() {
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    var q = Queue<float>()
    q.enqueue(5.5)
    println(f"{q.dequeue() ?? 0.0}")
    q.enqueue(10.5)
    q.enqueue(15.5)
    println(f"{q.dequeue() ?? 0.0}")
"#,
        "5.5
10.5",
    );
}
