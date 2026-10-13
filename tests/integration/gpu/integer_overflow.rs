// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Integer overflow inside a GPU kernel stops the program, as on the host.
//!
//! A kernel computes every integer in a 32-bit lane, so overflow is judged at
//! the width the device computes at: `int` is 32 bits on a GPU, and `i8`,
//! `i16`, `u8` and `u16` are checked at their own widths. Each checked
//! operation is spelled as a WGSL helper that sets a sticky device fault word
//! when its result does not fit; the host reports it at the next point device
//! data reaches it — a readback, a sync, or the end of the program.
//! Arithmetic the compiler synthesizes (thread indices, reduction strides) and
//! `wrapping_*` keep the plain WGSL operators and wrap.

use super::helpers::{assert_gpu_wgsl_valid, compile_to_wgsl};
use super::utils::{assert_runs_with_output, assert_runtime_error};
use crate::utils::miri_run;

const FAULT_DECLARATION: &str =
    "@group(1) @binding(0) var<storage, read_write> miri_fault: atomic<u32>;";

const OVERFLOW: &str = "integer overflow: the result does not fit its type";

/// A `forall` over a two-element `gpu` buffer of `element` seeded with
/// `seed`, storing `kernel_expression` back into each element, then reading
/// the buffer back and printing its second element.
fn kernel_program(element: &str, seed: &str, kernel_expression: &str) -> String {
    format!(
        "
use system.gpu
use system.math
use system.collections.array

fn main()
    gpu var buf = [({seed}) as {element}, ({seed}) as {element}]
    gpu forall i in 0..2
        buf[i] = {kernel_expression}
    let device = buf
    println(f\"{{device[1]}}\")
"
    )
}

#[test]
fn test_wgsl_kernel_addition_calls_a_checked_helper() {
    let wgsl = compile_to_wgsl(&kernel_program("int", "1", "buf[i] + 1"));
    assert!(wgsl.contains("_checked_add__i32(buf["), "{wgsl}");
    assert!(
        wgsl.contains("fn _checked_add__i32(a: i32, b: i32) -> i32 {"),
        "{wgsl}"
    );
    assert!(wgsl.contains(FAULT_DECLARATION), "{wgsl}");
    assert!(wgsl.contains("atomicOr(&miri_fault, 1u);"), "{wgsl}");
}

#[test]
fn test_wgsl_checked_helpers_follow_the_kernel_code() {
    let wgsl = compile_to_wgsl(&kernel_program("int", "1", "buf[i] + 1"));
    let entry = wgsl.find("@compute").expect("kernel entry point");
    let helper = wgsl.find("fn _checked_add__i32").expect("checked helper");
    let fault = wgsl.find(FAULT_DECLARATION).expect("fault declaration");
    assert!(entry < fault && fault < helper, "{wgsl}");
}

#[test]
fn test_wgsl_checked_helpers_are_sorted_by_name() {
    let wgsl = compile_to_wgsl(&kernel_program("int", "1", "buf[i] * 3 - 1"));
    let mul = wgsl.find("fn _checked_mul__i32").expect("mul helper");
    let sub = wgsl.find("fn _checked_sub__i32").expect("sub helper");
    assert!(mul < sub, "{wgsl}");
    assert_eq!(wgsl.matches(FAULT_DECLARATION).count(), 1, "{wgsl}");
}

#[test]
fn test_wgsl_wrapping_and_index_math_emit_plain_operators() {
    let wgsl = compile_to_wgsl(&kernel_program("int", "1", "wrapping_add(buf[i], 1)"));
    assert!(!wgsl.contains("miri_fault"), "{wgsl}");
    assert!(!wgsl.contains("_checked_"), "{wgsl}");
}

/// The thread index of a loop over a runtime range is arithmetic the
/// compiler writes, so it wraps rather than checks.
#[test]
fn test_wgsl_runtime_range_index_math_is_not_checked() {
    let wgsl = compile_to_wgsl(
        "
use system.gpu
use system.math
use system.collections.array

fn main()
    gpu var buf = Array<int, 8>()
    var s = 1
    var n = 8
    gpu forall i in s..n
        buf[i] = wrapping_add(i, 1)
",
    );
    assert!(!wgsl.contains("miri_fault"), "{wgsl}");
}

/// In a 2-D loop only the program's own `y * 2 + x` is checked: each helper
/// is defined once and called once, so neither axis index calls one.
#[test]
fn test_wgsl_two_axis_loop_checks_only_the_programs_index_expression() {
    let wgsl = compile_to_wgsl(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var buf = Array<int, 4>()
    gpu forall x, y in 0..2, 0..2
        buf[y * 2 + x] = 1
",
    );
    assert_eq!(wgsl.matches("_checked_add__i32(").count(), 2, "{wgsl}");
    assert_eq!(wgsl.matches("_checked_mul__i32(").count(), 2, "{wgsl}");
}

#[test]
fn test_wgsl_float_kernel_declares_no_fault_binding() {
    let wgsl = compile_to_wgsl(&kernel_program("f32", "1.5", "buf[i] * 2.0 + 1.0"));
    assert!(!wgsl.contains("miri_fault"), "{wgsl}");
}

#[test]
fn test_wgsl_reduction_index_math_is_not_checked() {
    let wgsl = compile_to_wgsl(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [1.5, 2.5, 3.0, 4.0]
    gpu let total = data.reduce(0.0, fn(a f32, b f32) f32: a + b)
    let host_total = total
    println(f'{host_total}')
",
    );
    assert!(!wgsl.contains("miri_fault"), "{wgsl}");
}

#[test]
fn test_wgsl_unsigned_division_is_not_checked() {
    let wgsl = compile_to_wgsl(&kernel_program("u32", "7", "buf[i] / 2 as u32"));
    assert!(!wgsl.contains("miri_fault"), "{wgsl}");
}

#[test]
fn test_wgsl_signed_division_and_remainder_are_checked() {
    let wgsl = compile_to_wgsl(&kernel_program(
        "int",
        "7",
        "buf[i] / buf[0] + buf[i] % buf[1]",
    ));
    assert!(wgsl.contains("_checked_div__i32("), "{wgsl}");
    assert!(wgsl.contains("_checked_rem__i32("), "{wgsl}");
}

/// A quotient or remainder by a constant other than `-1` always fits, so it
/// keeps the plain operator, and a kernel with no other checked operation
/// declares no fault word.
#[test]
fn test_wgsl_division_by_a_constant_declares_no_fault_word() {
    for expression in ["buf[i] / 2", "buf[i] % 3", "buf[i] / -2"] {
        let wgsl = compile_to_wgsl(&kernel_program("int", "7", expression));
        assert!(!wgsl.contains("_checked_"), "{expression}: {wgsl}");
        assert!(!wgsl.contains("miri_fault"), "{expression}: {wgsl}");
    }
}

#[test]
fn test_wgsl_division_by_minus_one_is_checked() {
    let wgsl = compile_to_wgsl(&kernel_program("int", "7", "buf[i] / -1"));
    assert!(wgsl.contains("_checked_div__i32("), "{wgsl}");
    let wgsl = compile_to_wgsl(&kernel_program("i8", "7", "buf[i] % (-1) as i8"));
    assert!(wgsl.contains("_checked_rem__i8("), "{wgsl}");
}

/// A three-way comparison subtracts two flags in `-1..=1`, which never
/// overflows, so a kernel that only compares declares no fault word.
#[test]
fn test_wgsl_three_way_comparison_is_not_checked() {
    let wgsl = compile_to_wgsl(&kernel_program("int", "7", "buf[i].compare(3)"));
    assert!(!wgsl.contains("miri_fault"), "{wgsl}");
}

/// The text a runtime looks for to recognise a kernel that may fault, read
/// from the runtime's source: the string literal of its `FAULT_DECLARATION`.
fn runtime_fault_key(source: &str) -> String {
    let start = source
        .find("FAULT_DECLARATION")
        .expect("the runtime names its fault declaration FAULT_DECLARATION");
    let literal = &source[start..];
    let open = literal.find('"').expect("an opening quote") + 1;
    let close = literal[open..].find('"').expect("a closing quote") + open;
    literal[open..close].to_string()
}

/// The native and web runtimes recognise a faulting kernel by the whole
/// declaration line the compiler emits, binding group and slot included.
#[test]
fn test_wgsl_fault_declaration_is_the_runtimes_search_key() {
    let wgsl = compile_to_wgsl(&kernel_program("int", "1", "buf[i] + 1"));
    let declaration = wgsl
        .lines()
        .find(|line| line.contains("miri_fault: atomic<u32>"))
        .expect("the kernel declares the fault word");
    for (runtime, source) in [
        (
            "native",
            include_str!("../../../src/runtime/gpu/src/fault.rs"),
        ),
        ("web", include_str!("../../../assets/web/miri-gpu.js")),
    ] {
        assert_eq!(runtime_fault_key(source), declaration, "{runtime} runtime");
    }
}

#[test]
fn test_wgsl_narrow_arithmetic_is_checked_at_its_own_width() {
    let wgsl = compile_to_wgsl(&kernel_program("u8", "10", "buf[i] + 250 as u8"));
    assert!(wgsl.contains("_checked_add__u8(buf["), "{wgsl}");
    assert!(wgsl.contains("extractBits(r, 0u, 8u)"), "{wgsl}");
}

#[test]
fn test_wgsl_negation_is_checked() {
    let wgsl = compile_to_wgsl(&kernel_program("i16", "5", "-buf[i]"));
    assert!(wgsl.contains("_checked_neg__i16(buf["), "{wgsl}");
}

/// Every checked helper of every lane passes naga's validator.
#[test]
fn test_wgsl_every_checked_helper_validates() {
    for element in ["i8", "i16", "i32", "int", "u8", "u16", "u32"] {
        assert_gpu_wgsl_valid(&kernel_program(
            element,
            "3",
            &format!("(buf[i] + buf[i] - (1) as {element}) * buf[i]"),
        ));
    }
    for element in ["i8", "i16", "i32", "int"] {
        assert_gpu_wgsl_valid(&kernel_program(
            element,
            "3",
            &format!("-(buf[i] / (2) as {element} + buf[i] % (2) as {element})"),
        ));
    }
    for element in ["u8", "u16", "u32"] {
        assert_gpu_wgsl_valid(&kernel_program(element, "0", "-buf[i]"));
    }
}

/// The smallest and largest value the device holds for `element`.
fn device_bounds(element: &str) -> (i64, i64) {
    match element {
        "i8" => (i64::from(i8::MIN), i64::from(i8::MAX)),
        "i16" => (i64::from(i16::MIN), i64::from(i16::MAX)),
        "i32" | "int" => (i64::from(i32::MIN), i64::from(i32::MAX)),
        "u8" => (0, i64::from(u8::MAX)),
        "u16" => (0, i64::from(u16::MAX)),
        "u32" => (0, i64::from(u32::MAX)),
        other => panic!("no device bounds for {other}"),
    }
}

/// `+`, `-`, `*` and unary `-` at the edges of `element`: one step past an
/// edge stops the program with an overflow, and the step that lands on the
/// edge computes its value.
fn assert_overflow_boundaries(element: &str) {
    let (min, max) = device_bounds(element);
    let one = format!("(1) as {element}");
    let two = format!("(2) as {element}");
    let add = format!("buf[i] + {one}");
    let sub = format!("buf[i] - {one}");
    let mul = format!("buf[i] * {two}");
    let (neg_overflow, neg_edge, neg_value) = if min < 0 {
        (min, min + 1, max)
    } else {
        (1, 0, 0)
    };
    for (seed, expression) in [
        (max, &add),
        (min, &sub),
        (max, &mul),
        (neg_overflow, &"-buf[i]".to_string()),
    ] {
        let program = kernel_program(element, &seed.to_string(), expression);
        assert_runtime_error(&program, OVERFLOW);
    }
    for (seed, expression, value) in [
        (max - 1, &add, max),
        (min + 1, &sub, min),
        (max / 2, &mul, max / 2 * 2),
        (neg_edge, &"-buf[i]".to_string(), neg_value),
    ] {
        let program = kernel_program(element, &seed.to_string(), expression);
        assert_runs_with_output(&program, &value.to_string());
    }
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i8_overflow_stops_the_program() {
    assert_overflow_boundaries("i8");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i16_overflow_stops_the_program() {
    assert_overflow_boundaries("i16");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_i32_overflow_stops_the_program() {
    assert_overflow_boundaries("i32");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_int_overflow_is_judged_at_thirty_two_bits() {
    assert_overflow_boundaries("int");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_u8_overflow_stops_the_program() {
    assert_overflow_boundaries("u8");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_u16_overflow_stops_the_program() {
    assert_overflow_boundaries("u16");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_u32_overflow_stops_the_program() {
    assert_overflow_boundaries("u32");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_min_divided_by_minus_one_stops_the_program() {
    for element in ["i8", "i16", "i32", "int"] {
        let min = device_bounds(element).0.to_string();
        let minus_one = format!("(-1) as {element}");
        for expression in [
            format!("buf[i] / {minus_one}"),
            format!("buf[i] % {minus_one}"),
        ] {
            let program = kernel_program(element, &min, &expression);
            assert_runtime_error(&program, OVERFLOW);
        }
    }
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_min_times_minus_one_stops_the_program() {
    for element in ["i8", "i32"] {
        let min = device_bounds(element).0.to_string();
        let program = kernel_program(element, &min, &format!("buf[i] * (-1) as {element}"));
        assert_runtime_error(&program, OVERFLOW);
    }
}

/// `65536 * 65536` wraps to exactly `0` in a 32-bit lane: a result whose sign
/// looks right and is not `MIN`, which only the quotient test catches.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_product_wrapping_to_zero_stops_the_program() {
    let program = kernel_program("i32", "65536", "buf[i] * buf[i]");
    assert_runtime_error(&program, OVERFLOW);
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_division_next_to_min_computes_its_value() {
    let seed = (i32::MIN + 1).to_string();
    let program = kernel_program("i32", &seed, "buf[i] / (-1) as i32");
    assert_runs_with_output(&program, &i32::MAX.to_string());
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_wrapping_add_still_wraps_on_the_device() {
    let program = kernel_program(
        "i32",
        &i32::MAX.to_string(),
        "wrapping_add(buf[i], 1 as i32)",
    );
    assert_runs_with_output(&program, &i32::MIN.to_string());
}

/// A reduction folds with the user's `+`, so a sum past the device `int`
/// stops the program when the scalar reaches the host.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_reduction_overflow_stops_the_program() {
    let code = "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [2147483647, 1, 0, 0]
    let sum = data.reduce(0, fn(a int, b int) int: a + b)
    println(f\"{sum}\")
";
    assert_runtime_error(code, OVERFLOW);
}

/// A reduction folding with `*` checks its product as `+` checks its sum.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_reduction_product_overflow_stops_the_program() {
    let code = "
use system.gpu
use system.collections.array

fn main()
    gpu var data = [65536, 65536, 1, 1]
    let product = data.reduce(1, fn(a int, b int) int: a * b)
    println(f\"{product}\")
";
    assert_runtime_error(code, OVERFLOW);
}

/// An explicitly launched `gpu fn` checks its arithmetic like a `forall`.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_fn_launch_overflow_stops_the_program() {
    let code = "
use system.gpu
use system.collections.array

gpu fn bump(dst out Array<int, 4>)
    let i = kernel.global_idx.x
    dst[i] = i + 2147483646

fn main()
    gpu var out = Array<int, 4>()
    bump(out).launch(Dim3(4, 1, 1), Dim3(1, 1, 1))
    let host = out
    println(f\"{host[0]}\")
";
    assert_runtime_error(code, OVERFLOW);
}

/// Every thread of a launch overflowing sets the same sticky bit, so the
/// program stops once, with one report.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_two_overflows_in_one_launch_are_reported_once() {
    let program = kernel_program("i32", &i32::MAX.to_string(), "buf[i] + (1) as i32");
    let output = miri_run(&program).output();
    assert_eq!(output.matches(OVERFLOW).count(), 1, "{output}");
}

/// The fault word belongs to the device, not to a buffer: reading back any
/// buffer reports an overflow another buffer's kernel recorded.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_readback_of_another_buffer_reports_the_overflow() {
    let code = "
use system.gpu
use system.collections.array

fn main()
    gpu var hot = [2147483647, 2147483647]
    gpu var calm = [1, 2]
    gpu forall i in 0..2
        hot[i] = hot[i] + 1
    gpu forall i in 0..2
        calm[i] = calm[i] * 2
    let host = calm
    println(f\"{host[1]}\")
";
    assert_runtime_error(code, OVERFLOW);
}

/// A `gpu`-resident launch never fences, so an overflow nothing reads back
/// is reported when the program ends.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_overflow_with_no_readback_is_reported_at_program_end() {
    let code = "
use system.gpu
use system.collections.array

fn main()
    gpu var buf = [2147483647, 2147483647]
    gpu forall i in 0..2
        buf[i] = buf[i] + 1
    println(\"launched\")
";
    assert_runtime_error(code, OVERFLOW);
}

/// A user `fn` called from a kernel checks its arithmetic too.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_overflow_in_a_kernel_callable_fn_stops_the_program() {
    let code = "
use system.gpu
use system.collections.array

fn bump(x int) int
    return x * 2

fn main()
    gpu var buf = [2147483647, 2147483647]
    gpu forall i in 0..2
        buf[i] = bump(buf[i])
    let device = buf
    println(f\"{device[1]}\")
";
    assert_runtime_error(code, OVERFLOW);
}

/// The minimum of a signed type has no magnitude at its width, so `abs` of it
/// in a kernel stops the program as it does on the host.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_abs_of_the_minimum_stops_the_program() {
    for element in ["i8", "i32"] {
        let min = device_bounds(element).0.to_string();
        let program = kernel_program(element, &min, "abs(buf[i])");
        assert_runtime_error(&program, OVERFLOW);
    }
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_abs_next_to_the_minimum_computes_its_value() {
    let program = kernel_program("i32", "-2147483647", "abs(buf[i])");
    assert_runs_with_output(&program, "2147483647");
}

#[test]
fn test_wgsl_signed_abs_is_checked_and_unsigned_abs_is_not() {
    let signed = compile_to_wgsl(&kernel_program("i32", "-5", "abs(buf[i])"));
    assert!(signed.contains("_checked_abs__i32(buf["), "{signed}");
    let unsigned = compile_to_wgsl(&kernel_program("u32", "5", "abs(buf[i])"));
    assert!(!unsigned.contains(FAULT_DECLARATION), "{unsigned}");
}
