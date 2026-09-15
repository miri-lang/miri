// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for the heap guard sanitizer (MIRI_HEAP_GUARD=1).
//
// The guard detects use-after-free and double-free bugs by tracking all
// allocations in a shadow table, poisoning freed blocks, and validating
// them on intrinsic entry.

use super::super::utils::*;
use crate::utils::miri_run_with_env;

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

/// A closure released after it was already freed is reported as a double free,
/// rather than the release reading the freed closure's reference count and
/// crashing, skipping, or freeing it again depending on what that memory holds.
///
/// The list holds a second reference, so dropping it releases the closure the
/// hook already freed.
#[test]
fn test_heap_guard_traps_closure_released_twice() {
    assert_heap_guard_detects(
        r#"
use system.collections.list
use system.testing

fn main()
    let tag = f"cap{1}"
    let f = fn() int: tag.length()
    let owners = List([f])
    simulate_closure_over_release(f)
    println(f"{owners.length()}")
"#,
        &["double-free detected", "allocated at", "first freed at"],
    );
}

/// A class element a list releases through its drop callback after the element
/// was already freed is reported by that callback, before it reads the freed
/// instance's reference count.
///
/// The program must stop at `clear`: a report only at the binding's later scope
/// exit would come from a different release and leave the callback unchecked,
/// so the output after `clear` is asserted absent.
#[test]
fn test_heap_guard_traps_class_element_released_twice_by_drop_callback() {
    let result = miri_run_with_env(
        r#"
use system.collections.list

class Box
    var n int

runtime "core" fn miri_rt_test_simulate_inline_over_release(b Box)

fn main()
    let b = Box(n: 7)
    var owners = List<Box>()
    owners.push(b)
    miri_rt_test_simulate_inline_over_release(b)
    println("released")
    owners.clear()
    println("cleared")
"#,
        "MIRI_HEAP_GUARD",
        "1",
    );
    assert!(
        !result.success && result.stderr.contains("double-free detected"),
        "expected a double-free report, got:\n{}",
        result.output()
    );
    assert_eq!(
        result.stdout.trim(),
        "released",
        "the report must come from `clear`:\n{}",
        result.output()
    );
}

/// An allocation compiled code makes inline is reported when it leaks, and the
/// report names it as a class rather than as an anonymous block.
///
/// This also pins the earliest allocation being tracked at all. Compiled code
/// asks a runtime byte whether reporting is wanted before it calls the tracking
/// hook, and that byte is only settled *by* the hook — so it starts out meaning
/// neither yes nor no, and only the value meaning "no" skips the call. Were the
/// initial value to mean "no", nothing would ever call in to settle it, every
/// allocation would go unseen, and this program would exit silently.
#[test]
fn test_heap_guard_reports_a_leaked_inline_allocation() {
    assert_heap_guard_detects(
        r#"
class Node
    public var peer Node?
    public fn init(): self.peer = None

fn main()
    var a = Node()
    a.peer = Some(a)
    println("built")
"#,
        &["leaked", "(class)"],
    );
}

/// A program the runtime is not observing allocates and releases correctly.
///
/// Compiled code reads a runtime byte and skips the reporting hooks when
/// nothing wants them, which is the path a released program takes and the one
/// every other test here misses: the harness runs each test with the leak
/// counter on, so tracking is always resolved to "report" elsewhere in the
/// suite. Allocating in a loop makes the branch run on both a hot path and a
/// release path rather than once.
#[test]
fn test_untracked_program_allocates_and_releases_correctly() {
    assert_runs_untracked(
        r#"
class Node
    public var value int
    public fn init(value int): self.value = value

fn main()
    var total = 0
    var index = 0
    while index < 1000
        let node = Node(index)
        total = total + node.value
        index = index + 1
    println(f"{total}")
"#,
        "499500",
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
