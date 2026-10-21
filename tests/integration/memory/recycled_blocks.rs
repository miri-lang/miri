// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A released small block is handed out again, uncleared, for the next value
//! of its size, so every constructor must write all of a value's state itself.
//! These programs release values and build new ones in the same blocks; a
//! constructor that left a field to the allocator would read what the previous
//! occupant wrote there.

use super::super::utils::*;
use crate::utils::miri_run_with_env_multiple;

/// Each round builds a list, a map, a set, an array and a string in the blocks
/// the previous round released, checks they start empty, then fills them.
const COLLECTIONS_IN_RELEASED_BLOCKS: &str = r#"
use system.io
use system.collections.list
use system.collections.map
use system.collections.set

fn faults_of_fresh_collections(n int) int
    var faults = 0
    var items = List<int>()
    var counts = Map<String, int>()
    var seen = Set<int>()
    if items.length() != 0 or counts.length() != 0 or seen.length() != 0: faults += 1
    if counts.contains_key("k0") or seen.contains(0): faults += 1
    var text = ""
    if text.length() != 0: faults += 1
    let fixed = [n, n + 1, n + 2]
    if fixed.length() != 3 or fixed[0] != n: faults += 1
    var i = 0
    while i < n
        items.push(i)
        counts.set(f"k{i}", i)
        seen.add(i)
        text = text + "x"
        i += 1
    if items.length() != n or counts.length() != n or seen.length() != n: faults += 1
    if text.length() != n: faults += 1
    faults

var faults = 0
var round = 1
while round <= 40
    faults += faults_of_fresh_collections(round)
    round += 1
println(f"{faults}")
"#;

#[test]
fn test_collections_built_in_released_blocks_start_empty() {
    assert_runs_with_output(COLLECTIONS_IN_RELEASED_BLOCKS, "0");
}

#[test]
fn test_collections_built_in_released_blocks_start_empty_untracked() {
    assert_runs_untracked(COLLECTIONS_IN_RELEASED_BLOCKS, "0");
}

#[test]
fn test_collections_built_in_released_blocks_pass_the_heap_guard() {
    assert_heap_guard_output(COLLECTIONS_IN_RELEASED_BLOCKS, "0");
}

/// A wide instance full of non-zero words is released and a narrower one of
/// the same size class takes its block: its optional field must read as the
/// `None` its initialiser stored, never as a pointer left by the wide one.
const INSTANCES_IN_RELEASED_BLOCKS: &str = r#"
use system.io

class Wide
    var a int
    var b int
    var c int
    var d int

    fn init(seed int)
        self.a = seed
        self.b = seed + 1
        self.c = seed + 2
        self.d = seed + 3

class Narrow
    var label String?
    var count int

    fn init(count int)
        self.label = None
        self.count = count

fn wide(seed int) Wide: Wide(seed)

fn narrow(count int) Narrow: Narrow(count)

var labelled = 0
var total = 0
var i = 0
while i < 200
    let w = wide(i * 7919 + 12345)
    total += w.d - w.a
    let n = narrow(i)
    match n.label
        Some(_): labelled += 1
        None: total += n.count - i
    i += 1
println(f"{labelled} {total}")
"#;

#[test]
fn test_instances_built_in_released_blocks_read_only_their_own_fields() {
    assert_runs_with_output(INSTANCES_IN_RELEASED_BLOCKS, "0 600");
}

#[test]
fn test_instances_built_in_released_blocks_pass_the_heap_guard() {
    assert_heap_guard_output(INSTANCES_IN_RELEASED_BLOCKS, "0 600");
}

/// Every instance is counted when it is made, though each takes the block the
/// previous one released.
#[test]
fn test_alloc_count_counts_each_instance_made_in_a_released_block() {
    let breakdown = parse_alloc_count_breakdown(INSTANCES_IN_RELEASED_BLOCKS);
    assert!(
        breakdown.inline >= 400,
        "expected one inline allocation per instance (400), got inline={}",
        breakdown.inline
    );
}

/// Two nodes keep each other alive, and each holds a list the runtime
/// allocated, while a loop releases and remakes lists of the same size.
const CYCLE_AMONG_RELEASED_BLOCKS: &str = r#"
use system.io
use system.collections.list

class Node
    var next Node?
    var items List<int>

    fn init()
        self.next = None
        self.items = List<int>()

fn churn() int
    var i = 0
    var made = 0
    while i < 100
        var items = List<int>()
        items.push(i)
        made += items.length()
        i += 1
    made

var a = Node()
var b = Node()
a.next = b
b.next = a
println(f"{churn()}")
"#;

/// A block the program never releases is still a leak while released blocks
/// of the same size wait to be handed out again. The heap guard is held off,
/// whatever the suite runs under, so the leak counter is what reports.
#[test]
fn test_leak_check_reports_a_leak_among_released_blocks() {
    let result = miri_run_with_env_multiple(
        CYCLE_AMONG_RELEASED_BLOCKS,
        &[("MIRI_HEAP_GUARD", "0"), ("MIRI_LEAK_CHECK", "1")],
    );
    assert!(
        !result.success,
        "the leak went unreported:\n{}",
        result.output()
    );
    assert!(
        result
            .stderr
            .contains("MIRI_LEAK_CHECK: leaked 2 RC allocation(s)"),
        "expected the two lists the cycle holds to be reported:\n{}",
        result.output()
    );
}

/// The heap guard attributes the same leak to the blocks themselves.
#[test]
fn test_heap_guard_reports_a_leak_among_released_blocks() {
    assert_heap_guard_detects(CYCLE_AMONG_RELEASED_BLOCKS, &["leaked 4 allocations"]);
}

/// The runtime under test as every suite run sees it — the leak check on, so
/// the cache poisons what it holds — with the heap guard held off whatever the
/// suite runs under: the guard bypasses the block cache, and these tests are
/// about what the cache does.
fn run_with_the_cache(code: &str, extra: &[(&str, &str)]) -> crate::utils::CompilerResult {
    let mut env = vec![("MIRI_HEAP_GUARD", "0"), ("MIRI_LEAK_CHECK", "1")];
    env.extend_from_slice(extra);
    miri_run_with_env_multiple(code, &env)
}

/// Released instances are handed out again: the count of blocks served from
/// released ones is reported, and is not zero.
#[test]
fn test_released_blocks_are_handed_out_again() {
    let result = run_with_the_cache(INSTANCES_IN_RELEASED_BLOCKS, &[("MIRI_ALLOC_COUNT", "1")]);
    assert!(result.success, "the program failed:\n{}", result.output());
    let reused: usize = result
        .stderr
        .lines()
        .find_map(|line| line.strip_prefix("MIRI_ALLOC_REUSED: "))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("no MIRI_ALLOC_REUSED line in:\n{}", result.stderr));
    assert!(
        reused > 0,
        "no block was handed out again:\n{}",
        result.stderr
    );
}

/// A block the runtime releases twice stops the program with a trap rather
/// than being handed to two owners.
#[test]
fn test_block_released_twice_traps() {
    let result = run_with_the_cache(
        r#"
use system.io
use system.testing

class Cell3
    var a int
    var b int
    var c int

    fn init(a int)
        self.a = a
        self.b = a
        self.c = a

simulate_double_free()
let x = Cell3(1)
let y = Cell3(2)
println(f"x.a={x.a} y.a={y.a}")
"#,
        &[],
    );
    assert!(
        !result.success,
        "the double release went unnoticed:\n{}",
        result.output()
    );
    assert!(
        result.stderr.contains("a heap block was released twice"),
        "expected the double-release trap:\n{}",
        result.output()
    );
}

/// A closure released one time too many is released again when its binding
/// goes out of scope, which writes the reference count of a block that is
/// already parked. The write is found when the block is handed out again.
#[test]
fn test_block_written_after_release_traps() {
    let result = run_with_the_cache(
        r#"
use system.io
use system.testing

fn over_release()
    let k = 5
    let f = fn() int: k + 1
    simulate_closure_over_release(f)

fn churn() int
    var total = 0
    var i = 0
    while i < 300
        let g = fn() int: i + 1
        total += g()
        i += 1
    total

over_release()
println(f"{churn()}")
"#,
        &[],
    );
    assert!(
        !result.success,
        "the write after release went unnoticed:\n{}",
        result.output()
    );
    assert!(
        result
            .stderr
            .contains("a heap block was written after it was released"),
        "expected the write-after-release trap:\n{}",
        result.output()
    );
}

/// A released closure read through a stale reference reads the cache's
/// poison, never the closure that took its block, so calling it crashes
/// instead of quietly running another closure's code.
#[test]
fn test_released_closure_called_through_a_stale_reference_crashes() {
    let result = run_with_the_cache(
        r#"
use system.io
use system.testing

fn main()
    let k = 5
    let f = fn() int: k + 1
    simulate_closure_over_release(f)
    let g = fn() int: k + 2
    println(f"{g()}")
    println(f"{f()}")
"#,
        &[],
    );
    assert!(!result.success, "the stale call ran:\n{}", result.output());
}
