// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use crate::utils::miri_run_with_env_multiple;

#[test]
fn test_class_instance_allocation() {
    let code = r#"
class Point
    var x i32
    var y i32

fn main():
    let p = Point(x: 10, y: 20)
    println("class created")
"#;
    // Class instances are allocated inline (not RC tracked via the RC module).
    // The Point(x: 10, y: 20) constructor allocates the struct via libc::malloc,
    // which codegen tracks via miri_rt_class_alloc_track, incrementing the inline counter once.
    // The println call uses string literals which are compile-time constants and allocate nothing.
    assert_allocation_count(code, 1);
}

#[test]
fn test_string_allocations() {
    let code = r#"
fn main():
    let s = "hello world"
    println(f"String: {s}")
"#;
    // The formatted string creates a new MiriString struct via alloc_with_rc,
    // which increments rc=1. The string data ("hello world") is then allocated
    // as raw storage, incrementing raw=1. The string literal "hello world" is
    // compile-time constant and allocates nothing. The f-string interpolation
    // creates the MiriString and its data buffer.
    // Total: rc=1 (struct) + raw=1 (data) = 2 allocations.
    assert_allocation_count(code, 2);
}

#[test]
fn test_alloc_count_disabled() {
    let code = r#"
class Point
    var x i32
    var y i32

fn main():
    let p = Point(x: 5, y: 10)
    println("test")
"#;
    // When MIRI_ALLOC_COUNT is not set, ensure:
    // 1. The program runs successfully
    // 2. No MIRI_ALLOC_COUNT line appears in stderr
    // 3. Leak check behavior is unchanged
    assert_allocation_count_disabled(code);
}

#[test]
fn test_zero_allocation_program() {
    let code = r#"
fn main():
    println("hello")
"#;
    // A program that allocates nothing must still report MIRI_ALLOC_COUNT: 0
    assert_allocation_count(code, 0);
}

#[test]
fn test_list_buffer_growth() {
    // Single push should have minimal buffer allocations
    let single_push = r#"
use system.collections.list

fn main()
    var l = List<i32>()
    l.push(1)
"#;
    let single = parse_alloc_count_breakdown(single_push);

    // 64 pushes should trigger multiple reallocations, increasing buffer count
    let many_pushes = r#"
use system.collections.list

fn main()
    var l = List<i32>()
    var i = 0
    while i < 64
        l.push(i)
        i += 1
"#;
    let many = parse_alloc_count_breakdown(many_pushes);

    assert!(
        many.buffers > single.buffers,
        "List growth should allocate more buffers: single={}, many={}",
        single.buffers,
        many.buffers
    );
}

#[test]
fn test_trap_1_inline_aggregate_counting() {
    let code = r#"
fn main()
    let tuple = (42, "test")
    println("tuple created")
"#;
    // Inline aggregates like tuples should be counted even with MIRI_LEAK_CHECK=0
    // and MIRI_HEAP_GUARD=0 (the "trap" being that they're only counted if
    // ensure_exit_handler_registered is called, which now happens at startup).
    let breakdown = parse_alloc_count_breakdown_with_env(
        code,
        &[
            ("MIRI_ALLOC_COUNT", "1"),
            ("MIRI_LEAK_CHECK", "0"),
            ("MIRI_HEAP_GUARD", "0"),
        ],
    );

    // Verify inline count is at least 1
    assert!(
        breakdown.inline >= 1,
        "Expected at least 1 inline allocation for tuple, but got inline={}",
        breakdown.inline
    );
}

#[test]
fn test_trap_2_both_count_and_leak_reported() {
    let code = r#"
use system.testing

fn main():
    simulate_closure_leak()
    println("leaked")
"#;
    let result =
        miri_run_with_env_multiple(code, &[("MIRI_ALLOC_COUNT", "1"), ("MIRI_LEAK_CHECK", "1")]);

    // Should fail with both leak and count reported
    if result.success {
        panic!(
            "Expected program to detect a leak and fail, but it exited successfully:\n{}",
            result.output()
        );
    }

    let stderr = &result.stderr;

    // Both messages must appear
    if !stderr.contains("MIRI_ALLOC_COUNT:") {
        panic!(
            "Expected MIRI_ALLOC_COUNT line in stderr when both counters are on, but got:\n{}",
            result.output()
        );
    }

    if !stderr.contains("MIRI_LEAK_CHECK:") {
        panic!(
            "Expected MIRI_LEAK_CHECK leak message in stderr, but got:\n{}",
            result.output()
        );
    }
}

#[test]
fn test_alloc_count_total_equals_sum_of_parts() {
    let code = r#"
use system.collections.list

fn main():
    var l = List<i32>()
    l.push(1)
    l.push(2)
    l.push(3)
    println("list created")
"#;
    let breakdown = parse_alloc_count_breakdown(code);

    // Verify that total == rc + inline + buffers + raw
    let calculated_total = breakdown.rc + breakdown.inline + breakdown.buffers + breakdown.raw;

    // Re-parse to get the total from the actual output
    use crate::utils::miri_run_with_env_multiple;
    let result =
        miri_run_with_env_multiple(code, &[("MIRI_ALLOC_COUNT", "1"), ("MIRI_LEAK_CHECK", "1")]);
    let line = result
        .stderr
        .lines()
        .find(|line| line.starts_with("MIRI_ALLOC_COUNT:"))
        .unwrap_or_else(|| panic!("Expected MIRI_ALLOC_COUNT line in {}", result.stderr));

    // Parse the total from the line: "MIRI_ALLOC_COUNT: X allocation(s) ..."
    let parts: Vec<&str> = line.split_whitespace().collect();
    let reported_total: usize = parts
        .get(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("Failed to parse total from: {}", line));

    assert_eq!(
        reported_total, calculated_total,
        "Total allocation count {} does not equal sum of parts: rc={} + inline={} + buffers={} + raw={}",
        reported_total, breakdown.rc, breakdown.inline, breakdown.buffers, breakdown.raw
    );
}
