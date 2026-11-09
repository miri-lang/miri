// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A GPU kernel stops the program where the host would: an integer `/` or
//! `%` by zero and an index outside its collection each set their own bit of
//! the device fault word, beside the overflow bit, and the host ends the run
//! at its next look with every kind that fired, each under the runtime code
//! the host's own trap reports. An index the compiler proved in range is
//! emitted without the check.

use super::helpers::{assert_gpu_wgsl_valid, compile_to_wgsl};
use super::utils::{assert_runs_with_output, assert_runtime_error};

const RAISE_DIVISION_BY_ZERO: &str = "atomicOr(&miri_fault, 2u);";
const RAISE_REMAINDER_BY_ZERO: &str = "atomicOr(&miri_fault, 4u);";
const RAISE_INDEX_OUT_OF_BOUNDS: &str = "atomicOr(&miri_fault, 8u);";

const DIVISION_BY_ZERO: &str = "division by zero (MER_RT_001)";
const REMAINDER_BY_ZERO: &str = "remainder by zero (MER_RT_002)";
const INDEX_OUT_OF_BOUNDS: &str = "index out of bounds (MER_RT_011)";

/// A `forall` over two-element `gpu` buffers `buf` of `element` and `d` of
/// `int`, storing `kernel_expression` into each element of `buf`, then
/// reading `buf` back and printing its first element.
fn kernel_program(element: &str, seed: &str, divisors: &str, kernel_expression: &str) -> String {
    format!(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var buf = [({seed}) as {element}, ({seed}) as {element}]
    gpu var d = [{divisors}]
    gpu forall i in 0..2
        buf[i] = {kernel_expression}
    let host = buf
    println(f\"{{host[0]}}\")
"
    )
}

#[test]
fn test_wgsl_division_by_a_runtime_divisor_records_division_by_zero() {
    let wgsl = compile_to_wgsl(&kernel_program("int", "7", "1, 0", "buf[i] / d[i]"));
    assert!(wgsl.contains("_checked_div__i32("), "{wgsl}");
    assert!(wgsl.contains(RAISE_DIVISION_BY_ZERO), "{wgsl}");
    assert!(!wgsl.contains(RAISE_REMAINDER_BY_ZERO), "{wgsl}");
}

#[test]
fn test_wgsl_remainder_by_a_runtime_divisor_records_remainder_by_zero() {
    let wgsl = compile_to_wgsl(&kernel_program("int", "7", "1, 0", "buf[i] % d[i]"));
    assert!(wgsl.contains("_checked_rem__i32("), "{wgsl}");
    assert!(wgsl.contains(RAISE_REMAINDER_BY_ZERO), "{wgsl}");
    assert!(!wgsl.contains(RAISE_DIVISION_BY_ZERO), "{wgsl}");
}

/// An unsigned quotient never overflows, but its divisor can still be zero.
#[test]
fn test_wgsl_unsigned_division_by_a_runtime_divisor_is_checked() {
    let wgsl = compile_to_wgsl(&kernel_program("u32", "7", "1, 0", "buf[i] / buf[1]"));
    assert!(wgsl.contains("_checked_div__u32("), "{wgsl}");
    assert!(wgsl.contains(RAISE_DIVISION_BY_ZERO), "{wgsl}");
    assert!(!wgsl.contains("atomicOr(&miri_fault, 1u);"), "{wgsl}");
}

/// A divisor written as a constant other than zero and `-1` can neither be
/// zero nor overflow, so the plain operator stays.
#[test]
fn test_wgsl_division_by_a_nonzero_constant_is_not_checked() {
    let wgsl = compile_to_wgsl(&kernel_program("int", "7", "1, 0", "buf[i] / 2"));
    assert!(!wgsl.contains("_checked_div"), "{wgsl}");
    assert!(!wgsl.contains(RAISE_DIVISION_BY_ZERO), "{wgsl}");
}

/// An index nothing proved in range is checked against its collection's
/// length before it reaches memory.
#[test]
fn test_wgsl_unproven_index_records_index_out_of_bounds() {
    let wgsl = compile_to_wgsl(&kernel_program("int", "7", "1, 0", "buf[d[i]]"));
    assert!(wgsl.contains("_checked_index__i32("), "{wgsl}");
    assert!(wgsl.contains(RAISE_INDEX_OUT_OF_BOUNDS), "{wgsl}");
    assert!(!wgsl.contains("clamp("), "{wgsl}");
}

/// A `gpu` buffer's length is the one its type declares, written into the
/// check rather than read off the device.
#[test]
fn test_wgsl_index_check_reads_a_fixed_buffers_declared_length() {
    let wgsl = compile_to_wgsl(&kernel_program("int", "7", "1, 0", "buf[d[i]]"));
    assert!(wgsl.contains(", 2u)]"), "{wgsl}");
}

/// Every checked helper a faulting kernel can call passes naga's validator.
#[test]
fn test_wgsl_fault_checks_validate() {
    for element in ["i8", "i16", "i32", "int", "u8", "u16", "u32"] {
        assert_gpu_wgsl_valid(&kernel_program(
            element,
            "7",
            "1, 0",
            "buf[i] / buf[1] + buf[i] % buf[1] + buf[d[i]]",
        ));
    }
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_division_by_zero_stops_the_program() {
    for element in ["int", "i8", "u32"] {
        let program = kernel_program(
            element,
            "7",
            "1, 0",
            &format!("buf[i] / (d[i]) as {element}"),
        );
        assert_runtime_error(&program, DIVISION_BY_ZERO);
    }
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_remainder_by_zero_stops_the_program() {
    for element in ["int", "u16"] {
        let program = kernel_program(
            element,
            "7",
            "1, 0",
            &format!("buf[i] % (d[i]) as {element}"),
        );
        assert_runtime_error(&program, REMAINDER_BY_ZERO);
    }
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_index_past_the_end_stops_the_program() {
    let program = kernel_program("int", "7", "0, 5", "buf[d[i]]");
    assert_runtime_error(&program, INDEX_OUT_OF_BOUNDS);
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_negative_index_stops_the_program() {
    let program = kernel_program("int", "7", "0, 1", "buf[d[i] - 1]");
    assert_runtime_error(&program, INDEX_OUT_OF_BOUNDS);
}

/// One launch tripping two kinds of fault reports both, in one error.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_two_kinds_of_fault_in_one_launch_are_both_reported() {
    let program = kernel_program("int", "7", "0, 5", "buf[i] / d[i] + buf[d[i]]");
    assert_runtime_error(
        &program,
        &format!("a GPU kernel faulted: {DIVISION_BY_ZERO}; {INDEX_OUT_OF_BOUNDS}"),
    );
}

/// A kernel whose divisors are not zero and whose indexes are in range
/// computes its values and reports nothing.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_in_range_kernel_reports_no_fault() {
    let program = kernel_program(
        "int",
        "7",
        "1, 2",
        "buf[i] / d[i] + buf[i] % d[i] + buf[d[i] - 1]",
    );
    assert_runs_with_output(&program, "14");
}

/// The browser runtime's copy of the fault table: each `{ bit: …, code: "…",
/// sentence: "…" }` entry of its `FAULT_KINDS`, in order.
fn web_fault_kinds(js: &str) -> Vec<(u32, String, String)> {
    let start = js
        .find("const FAULT_KINDS = [")
        .expect("the web runtime declares FAULT_KINDS");
    let end = start + js[start..].find("];").expect("FAULT_KINDS is closed");
    js[start..end]
        .lines()
        .filter(|line| line.contains("bit:"))
        .map(|line| {
            let field = |name: &str| {
                let rest = &line[line.find(name).expect(name) + name.len()..];
                rest.trim_start()
                    .split([',', '}'])
                    .next()
                    .expect("a value")
                    .trim()
                    .trim_matches('"')
                    .to_string()
            };
            let bit = field("bit:");
            let bit = u32::from_str_radix(bit.trim_start_matches("0x"), 16).expect("a hex bit");
            (bit, field("code:"), field("sentence:"))
        })
        .collect()
}

/// The string literal `const name = "…";` holds in the web runtime.
fn web_string_constant(js: &str, name: &str) -> String {
    let marker = format!("const {name} = \"");
    let start = js
        .find(&marker)
        .unwrap_or_else(|| panic!("{name} declared"))
        + marker.len();
    let end = start + js[start..].find('"').expect("closing quote");
    js[start..end].to_string()
}

/// The browser runtime reads the fault word with the same table the host
/// does: every bit, in order, with the host's code and sentence, and the
/// same report around them.
#[test]
fn test_web_runtime_fault_table_matches_the_compilers() {
    use miri::abi::gpu_fault::{GpuFault, SUMMARY_PREFIX, SUMMARY_SEPARATOR};
    let js = include_str!("../../../assets/web/miri-gpu.js");
    let expected: Vec<(u32, String, String)> = GpuFault::ALL
        .into_iter()
        .map(|fault| {
            (
                fault.bit(),
                fault.code().to_string(),
                fault.sentence().to_string(),
            )
        })
        .collect();
    assert_eq!(web_fault_kinds(js), expected);
    assert_eq!(
        web_string_constant(js, "FAULT_SUMMARY_PREFIX"),
        SUMMARY_PREFIX
    );
    assert_eq!(
        web_string_constant(js, "FAULT_SUMMARY_SEPARATOR"),
        SUMMARY_SEPARATOR
    );
}

/// A `forall` over two lanes, each adding 1 to the element of the two-element
/// atomic buffer `hist` that `targets` names for it, then printing both
/// elements.
fn atomic_program(targets: &str) -> String {
    format!(
        "
use system.gpu
use system.gpu.atomic
use system.collections.array

fn main()
    gpu var hist = Array<Atomic<u32>, 2>()
    gpu var d = [{targets}]
    gpu forall i in 0..2
        atomic_add(hist, d[i], 1 as u32)
    let host = hist
    println(f\"{{host[0]}} {{host[1]}}\")
"
    )
}

/// An atomic's element index is checked against the buffer's length like any
/// other index nothing proved in range.
#[test]
fn test_wgsl_atomic_on_an_unproven_index_records_index_out_of_bounds() {
    let wgsl = compile_to_wgsl(&atomic_program("1, 0"));
    assert!(wgsl.contains("atomicAdd(&hist[_checked_index__"), "{wgsl}");
    assert!(wgsl.contains(RAISE_INDEX_OUT_OF_BOUNDS), "{wgsl}");
}

#[test]
fn test_wgsl_checked_atomic_index_validates() {
    assert_gpu_wgsl_valid(&atomic_program("1, 0"));
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_atomic_past_the_end_stops_the_program() {
    assert_runtime_error(&atomic_program("0, 5"), INDEX_OUT_OF_BOUNDS);
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_in_range_atomic_reports_no_fault() {
    assert_runs_with_output(&atomic_program("1, 1"), "0 2");
}
