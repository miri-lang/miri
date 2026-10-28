// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Loops compared with a bound other than the list's length. The compiler
//! copies such a loop and runs the copy without bounds checks when a guard
//! finds the bound within the length of every list the loop indexes; when
//! it does not, the loop runs as written and stops at the first access past
//! an end with the usual report. Every program here runs under the heap
//! guard, and the traps also in a release build.

use super::utils::*;
use crate::utils::{miri_cmd, miri_run_with_env, CompilerResult};
use std::io::Write;

fn program(body: &str) -> String {
    format!("use system.io\nuse system.collections.list\n\n{body}")
}

/// `source` run as a release build: optimized, without the heap guard.
fn run_released(source: &str) -> CompilerResult {
    let mut file = tempfile::NamedTempFile::new().expect("a temporary file");
    write!(file, "{source}").expect("the program is written");
    let stdlib = concat!(env!("CARGO_MANIFEST_DIR"), "/src/stdlib");
    let output = miri_cmd()
        .env("MIRI_STDLIB_PATH", stdlib)
        .env("MIRI_VERIFY_MIR", "1")
        .env_remove("MIRI_CC")
        .env_remove("CC")
        .arg("run")
        .arg(file.path())
        .arg("--release")
        .output()
        .expect("the compiler runs");
    CompilerResult {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

fn expect_output(body: &str, expected: &str) {
    let source = program(body);
    assert_runs_with_output(&source, expected);
    assert_heap_guard_output(&source, expected);
    let released = run_released(&source);
    assert!(
        released.success && released.stdout.trim() == expected.trim(),
        "in a release build:\n{}",
        released.output()
    );
}

fn expect_out_of_bounds(body: &str, report: &str) {
    let source = program(body);
    assert_runtime_error(&source, report);
    let guarded = miri_run_with_env(&source, "MIRI_HEAP_GUARD", "1");
    assert!(
        !guarded.success && guarded.stderr.contains(report),
        "under the heap guard:\n{}",
        guarded.output()
    );
    let released = run_released(&source);
    assert!(
        !released.success && released.output().contains(report),
        "in a release build:\n{}",
        released.output()
    );
}

const SUM_BELOW_A_BOUND: &str = r#"
fn total(l [int], n int) int
    var i = 0
    var t = 0
    while i < n
        t = t + l[i]
        i += 1
    t
"#;

#[test]
fn test_a_sum_below_a_bound_equal_to_the_length_reads_every_element() {
    expect_output(
        &format!(
            "{SUM_BELOW_A_BOUND}
fn main()
    let l = List([1, 2, 3, 4])
    println(f\"{{total(l, 4)}} {{total(l, 2)}} {{total(l, 0)}}\")
"
        ),
        "10 3 0",
    );
}

#[test]
fn test_a_sum_below_a_bound_past_a_resized_list_stops_at_its_end() {
    expect_out_of_bounds(
        &format!(
            "{SUM_BELOW_A_BOUND}
fn main()
    var l = List<int>()
    l.resize(5, 0)
    println(f\"{{total(l, 10)}}\")
"
        ),
        "the len is 5 but the index is 5",
    );
}

#[test]
fn test_a_loop_up_to_a_bound_below_the_length_reads_every_element() {
    expect_output(
        r#"
fn total(l [int], n int) int
    var i = 0
    var t = 0
    while i <= n
        t = t + l[i]
        i += 1
    t

fn main()
    let l = List([1, 2, 3, 4])
    println(f"{total(l, 3)} {total(l, 0)}")
"#,
        "10 1",
    );
}

#[test]
fn test_a_loop_up_to_a_bound_equal_to_the_length_stops_one_past_the_end() {
    expect_out_of_bounds(
        r#"
fn total(l [int], n int) int
    var i = 0
    var t = 0
    while i <= n
        t = t + l[i]
        i += 1
    t

fn main()
    let l = List([1, 2, 3])
    println(f"{total(l, l.length())}")
"#,
        "the len is 3 but the index is 3",
    );
}

#[test]
fn test_a_list_a_callee_shrinks_inside_the_loop_still_stops_the_program() {
    expect_out_of_bounds(
        r#"
fn without_last(l [int]) [int]
    var shorter = l
    shorter.pop()
    shorter

fn main()
    var l = List([1, 2, 3, 4])
    let n = 4
    var i = 0
    var t = 0
    while i < n
        t = t + l[i]
        l = without_last(l)
        i += 1
    println(f"{t}")
"#,
        "the len is 2 but the index is 2",
    );
}

#[test]
fn test_a_list_popped_inside_the_loop_still_stops_the_program() {
    expect_out_of_bounds(
        r#"
fn main()
    var l = List([1, 2, 3, 4])
    let n = 4
    var i = 0
    var t = 0
    while i < n
        t = t + l[i]
        l.pop()
        i += 1
    println(f"{t}")
"#,
        "the len is 2 but the index is 2",
    );
}

/// A loop copying the first `n` elements of one list into another of
/// `size` elements, as the permutation copy of fannkuch-redux does.
fn copy_prefix(size: usize) -> String {
    format!(
        r#"
fn copy_prefix(n int) int
    let source = List([5, 6, 7])
    var into = List<int>()
    into.resize({size}, 0)
    var k = 0
    while k < n
        into[k] = source[k]
        k += 1
    into[0] + into[into.length() - 1]

fn main()
    println(f"{{copy_prefix(3)}} {{copy_prefix(1)}}")
"#
    )
}

#[test]
fn test_a_copy_between_two_lists_below_a_bound_copies_every_element() {
    expect_output(&copy_prefix(3), "12 5");
}

#[test]
fn test_a_copy_into_a_shorter_list_stops_at_its_end() {
    expect_out_of_bounds(&copy_prefix(2), "the len is 2 but the index is 2");
}

#[test]
fn test_the_sieve_counts_the_primes_below_a_parameter() {
    expect_output(
        r#"
fn primes(n int) int
    var composite = List<bool>()
    composite.resize(n + 1, false)
    var count = 0
    var p = 2
    while p <= n
        if not composite[p]
            count += 1
            var m = p * p
            while m <= n
                composite[m] = true
                m += p
        p += 1
    count

fn main()
    println(f"{primes(100)} {primes(2)} {primes(1)}")
"#,
        "25 1 0",
    );
}

/// A list of `1..=n` with the elements from 0 to `hi` reversed by two
/// pointers meeting in the middle, as fannkuch-redux flips its prefix.
fn reversal(n: usize, hi: usize) -> String {
    format!(
        r#"
fn reversed(n int, last int) [int]
    var l = List<int>()
    var i = 0
    while i < n
        l.push(i + 1)
        i += 1
    var lo = 0
    var hi = last
    while lo < hi
        let t = l[lo]
        l[lo] = l[hi]
        l[hi] = t
        lo += 1
        hi -= 1
    l

fn main()
    let r = reversed({n}, {hi})
    println(f"{{r[0]}} {{r[1]}} {{r[2]}} {{r[3]}} {{r[4]}}")
"#
    )
}

#[test]
fn test_a_reversal_by_two_pointers_reverses_the_whole_list() {
    expect_output(&reversal(5, 4), "5 4 3 2 1");
}

#[test]
fn test_a_reversal_by_two_pointers_reverses_a_prefix() {
    expect_output(&reversal(5, 2), "3 2 1 4 5");
}

#[test]
fn test_a_reversal_starting_one_past_the_end_stops_there() {
    expect_out_of_bounds(&reversal(5, 5), "the len is 5 but the index is 5");
}

/// A loop shifting the first `n` elements of a list of 4 one place to the
/// left, reading one past its index, as fannkuch-redux rotates its
/// permutation.
fn shift_left(n: usize) -> String {
    format!(
        r#"
fn shifted(n int) [int]
    var l = List([1, 2, 3, 4])
    var k = 0
    while k < n
        l[k] = l[k + 1]
        k += 1
    l

fn main()
    let r = shifted({n})
    println(f"{{r[0]}} {{r[1]}} {{r[2]}} {{r[3]}}")
"#
    )
}

#[test]
fn test_a_shift_reading_one_past_its_index_below_the_last_element_shifts_every_element() {
    expect_output(&shift_left(3), "2 3 4 4");
}

#[test]
fn test_a_shift_reading_one_past_its_index_up_to_the_length_stops_one_past_the_end() {
    expect_out_of_bounds(&shift_left(4), "the len is 4 but the index is 4");
}

#[test]
fn test_an_index_past_a_bound_that_took_the_lists_length_stops_at_its_end() {
    // `x < n` holds of `x` before `x = n`; after it, `p < x` leaves `p + 1`
    // free to reach the length.
    expect_out_of_bounds(
        r#"
fn main()
    let a = List([1, 2, 3, 4, 5, 6])
    let b = List([1, 2, 3, 4, 5])
    var total = 0
    var r = 0
    while r < 3
        let n = a.length()
        var x = 0
        let p = b.length()
        if x < n
            x = n
            if p < x
                let q = p + 1
                total = total + a[q]
        r += 1
    println(f"{total}")
"#,
        "the len is 6 but the index is 6",
    );
}

#[test]
fn test_an_index_past_a_bound_copied_through_a_chain_stops_at_its_end() {
    // The length reaches `x` through `y` and `z`; nothing known of the
    // earlier values of any of them may survive into `p < x`.
    expect_out_of_bounds(
        r#"
fn main()
    let a = List([1, 2, 3, 4, 5, 6])
    let b = List([1, 2, 3, 4, 5])
    var total = 0
    var r = 0
    while r < 3
        let n = a.length()
        let p = b.length()
        var x = 0
        var y = 0
        var z = 0
        if x < n
            y = n
            z = y
            x = z
            if p < x
                let q = p + 1
                total = total + a[q]
        r += 1
    println(f"{total}")
"#,
        "the len is 6 but the index is 6",
    );
}

#[test]
fn test_an_index_past_a_bound_swapped_through_a_temporary_stops_at_its_end() {
    // `x < y` holds before the swap; after it, `y` is the smaller value and
    // `x` the length, so `p + 1` reaches the length through `x`.
    expect_out_of_bounds(
        r#"
fn main()
    let a = List([1, 2, 3, 4, 5, 6])
    let b = List([1, 2, 3, 4, 5])
    var total = 0
    var r = 0
    while r < 3
        let n = a.length()
        let p = b.length()
        var x = 0
        var y = n
        var t = 0
        if x < y
            t = x
            x = y
            y = t
            if p < y
                let q = p + 1
                total = total + a[q]
            if p < x
                let q = p + 1
                total = total + a[q]
        r += 1
    println(f"{total}")
"#,
        "the len is 6 but the index is 6",
    );
}

#[test]
fn test_a_loop_whose_bound_is_copied_back_from_the_length_stops_at_its_end() {
    // `k < n` holds before `k = n; n = k`; the loop reads one past `i`, so
    // its last pass reaches the length and must stop there.
    expect_out_of_bounds(
        r#"
fn main()
    let a = List([1, 2, 3, 4, 5, 6])
    var total = 0
    var r = 0
    while r < 3
        var n = a.length()
        var i = 0
        var k = 0
        if k < n
            k = n
            n = k
            while i < n
                total = total + a[i + 1]
                i += 1
        r += 1
    println(f"{total}")
"#,
        "the len is 6 but the index is 6",
    );
}
