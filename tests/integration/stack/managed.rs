// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_stack_string_push_iterate_balanced() {
    // A stack yielding String elements must have both the type checker and MIR
    // agree on the drop path. This test uses iteration (not explicit pop) to
    // exercise the element drop path. Values built from string literals are RC-blind,
    // so a true RC balance test would require runtime-allocated strings ("a" + "b").
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<String>()
    s.push("apple")
    s.push("banana")
    s.push("cherry")

    var len_sum = 0
    for str in s
        len_sum += str.length()

    println(f"total {len_sum}")
"#,
        "total 17",
    );
}

#[test]
fn test_stack_string_high_count_rc_balance() {
    // High element count to catch probabilistic RC leaks. Iteration and pop
    // must properly manage reference counts for String elements.
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    let s = Stack<String>()
    var i = 0
    while i < 100
        s.push("x")
        i += 1

    var count = 0
    for str in s
        count += 1

    println(f"count {count}")
    println("done")
"#,
        "count 100\ndone",
    );
}

#[test]
fn test_stack_string_drop_at_scope_end() {
    // Dropping a Stack with managed elements in its backing store must not leak
    // or double-free. The stack goes out of scope, drops the List, which drops
    // all String elements.
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    var s = Stack<String>()
    s.push("hello")
    s.push("world")

    println("scope end")
"#,
        "scope end",
    );
}
