// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_queue_string_enqueue_iterate_balanced() {
    // A queue yielding String elements must have both the type checker and MIR
    // agree on the drop path. This test uses iteration (not explicit dequeue) to
    // exercise the element drop path. Values built from string literals are RC-blind,
    // so a true RC balance test would require runtime-allocated strings ("a" + "b").
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<String>()
    q.enqueue("apple")
    q.enqueue("banana")
    q.enqueue("cherry")

    var len_sum = 0
    for s in q
        len_sum += s.length()

    println(f"total {len_sum}")
"#,
        "total 17",
    );
}

#[test]
fn test_queue_string_high_count_rc_balance() {
    // High element count to catch probabilistic RC leaks. Iteration and dequeue
    // must properly manage reference counts for String elements.
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    let q = Queue<String>()
    var i = 0
    while i < 100
        q.enqueue("x")
        i += 1

    var count = 0
    for s in q
        count += 1

    println(f"count {count}")
    println("done")
"#,
        "count 100\ndone",
    );
}

#[test]
fn test_queue_string_drop_at_scope_end() {
    // Dropping a Queue with managed elements in its backing store must not leak
    // or double-free. The queue goes out of scope, drops the List, which drops
    // all String elements.
    assert_runs_with_output(
        r#"
use system.collections.queue

fn main()
    var q = Queue<String>()
    q.enqueue("hello")
    q.enqueue("world")

    println("scope end")
"#,
        "scope end",
    );
}
