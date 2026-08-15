// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for the heap guard sanitizer (MIRI_HEAP_GUARD=1).
//
// The guard detects use-after-free and double-free bugs by tracking all
// allocations in a shadow table, poisoning freed blocks, and validating
// them on intrinsic entry.

use super::super::utils::*;

/// A normal program using lists, maps, strings runs clean under MIRI_HEAP_GUARD=1.
/// The most important test: a sanitizer that false-positives on correct code is worthless.
#[test]
fn test_heap_guard_clean_program() {
    assert_heap_guard_ok(
        r#"
use system.collections.list
use system.collections.map

fn main()
    var numbers = List<int>([1, 2, 3])
    numbers.push(4)
    let len = numbers.length()
    println(f"Length: {len}")

    var data = Map<String, int>()
    data.set("x", 10)
    data.set("y", 20)
    println("Data: x set")

    let msg = "Hello, World!"
    println(msg)
"#,
    );
}

/// A deliberate double free is trapped, and the report names the allocation
/// site and BOTH free sites.
///
/// This is the case the counter-based leak check is structurally blind to: with
/// the guard off the same program dies as a signal-killed child (exit 255) with
/// no diagnostic at all, because the process is gone before the atexit handler
/// could run.
#[test]
fn test_heap_guard_traps_double_free() {
    assert_heap_guard_detects(
        r#"
use system.io
use system.testing

fn main()
    simulate_double_free()
    println("survived")
"#,
        &[
            "double-free detected",
            "allocated at",
            "first freed at",
            "freed again at",
        ],
    );
}

/// Guard output is absent when the variable is unset (no behavior change).
#[test]
fn test_heap_guard_disabled_by_default() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var lst = List<int>([1, 2, 3])
    lst.push(4)
    println("Done")
"#,
        "Done",
    );
}
