// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! WGSL backend shader-validity tests.
//!
//! Each test exercises a small `gpu for` kernel through the WGSL backend and
//! `naga`'s validator to confirm the shader source is syntactically and
//! type-correctly generated. These tests require no GPU hardware.
//!
//! Value-correctness testing (i.e., comparing computed values against
//! expected results) is owned by `super::launch` via the compiler-driven
//! native dispatch path.

use super::helpers::assert_gpu_wgsl_valid;

#[test]
fn vector_add_emits_naga_valid_wgsl() {
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [10, 20, 30, 40]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = a[i] + b[i]
",
    );
}

#[test]
fn float_kernel_emits_naga_valid_wgsl() {
    // F32 arithmetic in a `gpu for` body. Naga rejects type or alignment
    // mistakes in float storage bindings, so this exercises the F32
    // half of the scalar mapping independently of the integer paths.
    // Float literals that round-trip in f32 are typed as `Array<f32, N>`
    // by the parser (see project memory: "Float layout in collections"),
    // so the unannotated literal binds the kernel buffers as f32 storage.
    assert_gpu_wgsl_valid(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu let b = [0.5, 1.5, 2.5, 3.5]
    gpu var dst = [0.0, 0.0, 0.0, 0.0]
    gpu forall i in 0..4
        dst[i] = a[i] + b[i]
",
    );
}

/// Per-(scalar, op-kind) coverage grid for the WGSL backend.
///
/// For every Miri primitive that today round-trips into a WGSL storage buffer
/// element width, there is at least one `gpu for` kernel exercising the
/// scalar against a representative arithmetic op. For every primitive the
/// backend cannot represent today (bool buffers, heap-only types), there is
/// an `assert_compiler_error` proving the diagnostic fires before MIR
/// lowering — not as a naga validation failure on emitted shader source.
mod types {
    use super::super::utils::assert_compiler_error;
    use super::assert_gpu_wgsl_valid;

    #[test]
    fn int_buffer_add_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [10, 20, 30, 40]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = a[i] + b[i]
",
        );
    }

    #[test]
    fn int_buffer_mul_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let src = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = src[i] * 7
",
        );
    }

    #[test]
    fn int_buffer_sub_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [100, 200, 300, 400]
    gpu let b = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = a[i] - b[i]
",
        );
    }

    /// i64 div/mod: operands are narrowed to i32 for the op then widened back.
    /// This works around naga MSL's "select is ambiguous" error on Metal.
    #[test]
    fn int_buffer_div_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [0, 1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0, 0]
    gpu forall i in 0..5
        dst[i] = a[i] / 2
",
        );
    }

    #[test]
    fn int_buffer_rem_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [0, 1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0, 0]
    gpu forall i in 0..5
        dst[i] = a[i] % 3
",
        );
    }

    #[test]
    fn f32_buffer_add_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu let b = [0.5, 1.5, 2.5, 3.5]
    gpu var dst = [0.0, 0.0, 0.0, 0.0]
    gpu forall i in 0..4
        dst[i] = a[i] + b[i]
",
        );
    }

    #[test]
    fn f32_buffer_mul_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu var dst = [0.0, 0.0, 0.0, 0.0]
    gpu forall i in 0..4
        dst[i] = a[i] * 2.5
",
        );
    }

    /// f64 literals (don't round-trip in f32) bind storage buffers as
    /// `array<f64>`. WGSL has no `enable` extension for 64-bit scalars —
    /// naga gates `f64` through validator `Capabilities::FLOAT64` and the
    /// device gates it through `Features::SHADER_F64`, so the shader source
    /// carries no directive. naga still rejects the kernel if the storage
    /// elements disagree on width.
    #[test]
    fn f64_buffer_add_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [3.141592653589793, 2.718281828459045, 1.4142135623730951, 0.5772156649015329]
    gpu let b = [3.141592653589793, 2.718281828459045, 1.4142135623730951, 0.5772156649015329]
    gpu var dst = [3.141592653589793, 2.718281828459045, 1.4142135623730951, 0.5772156649015329]
    gpu forall i in 0..4
        dst[i] = a[i] + b[i]
",
        );
    }

    /// WGSL forbids `bool` in `var<storage>` bindings, so a captured
    /// `[true, false, ...]` array is rejected with a single source-cited
    /// diagnostic before MIR lowering.
    #[test]
    fn bool_storage_buffer_is_rejected_before_mir() {
        assert_compiler_error(
            "
use system.gpu
use system.collections.array

gpu var flags = [true, false, true, false]
gpu forall i in 0..4
    flags[i] = not flags[i]
",
            "not a valid WGSL storage-buffer element",
        );
    }

    /// String buffers have no storage layout on the device. Rejection
    /// reaches the user as the same buffer-element diagnostic class so
    /// the failure surface is uniform across non-mappable scalars.
    #[test]
    fn string_storage_buffer_is_rejected_before_mir() {
        assert_compiler_error(
            "
use system.gpu
use system.collections.array

gpu var labels = [\"a\", \"b\", \"c\", \"d\"]
gpu forall i in 0..4
    let _ = labels[i]
",
            "not a valid WGSL storage-buffer element",
        );
    }
}

mod loops {
    use super::assert_gpu_wgsl_valid;

    /// A counted inner for-loop inside the kernel body. The kernel accumulates
    /// a partial sum over a small region (4 elements) of a larger array using
    /// an inner loop. This exercises loop structurization and tests that the
    /// inner loop's induction variable (j) is correctly cast when indexing.
    #[test]
    fn inner_for_loop_reduction_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4, 5, 6, 7, 8]
    gpu var dst = [0, 0]
    gpu forall i in 0..2
        var s = 0
        for j in 0..4
            s = s + a[i * 4 + j]
        dst[i] = s
",
        );
    }

    /// A while loop inside the kernel body. Tests that while-loop structurization
    /// emits naga-valid WGSL.
    #[test]
    fn while_loop_inside_kernel_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4, 5, 6, 7, 8]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var j = 0
        while j < 2
            dst[i] = dst[i] + a[i * 2 + j]
            j = j + 1
",
        );
    }

    /// A break statement inside an inner loop. Tests that break inside a
    /// counted loop correctly maps to WGSL break.
    #[test]
    fn inner_loop_with_break_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var j = 0
        for j in 0..4
            dst[i] = a[i]
            break
",
        );
    }

    /// A continue statement inside an inner for loop. Tests that continue
    /// inside a counted loop correctly skips to the increment.
    #[test]
    fn inner_for_loop_with_continue_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4, 5, 6, 7, 8]
    gpu var dst = [0, 0]
    gpu forall i in 0..2
        var s = 0
        for j in 0..4
            if j == 1
                continue
            s = s + a[i * 4 + j]
        dst[i] = s
",
        );
    }

    /// A nested loop: loop inside a loop. Tests that break/continue are
    /// correctly scoped to the innermost loop.
    #[test]
    fn nested_loops_inside_kernel_emit_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
    gpu var dst = [0, 0]
    gpu forall i in 0..2
        var s = 0
        for j in 0..2
            for k in 0..3
                s = s + a[i * 6 + j * 3 + k]
        dst[i] = s
",
        );
    }

    /// An if statement inside a loop inside the kernel body. Tests that if
    /// and loop control flow composes correctly.
    #[test]
    fn if_inside_loop_inside_kernel_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var s = 0
        for j in 0..2
            if j == 0
                s = s + a[i]
        dst[i] = s
",
        );
    }

    /// An early return statement inside a loop inside the kernel body.
    /// Tests that return inside a loop terminates the entry point correctly.
    #[test]
    fn return_inside_loop_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        for j in 0..2
            dst[i] = a[i]
            return
",
        );
    }
}

mod math_intrinsics {
    use super::super::device::assert_gpu_runs_with_output;
    use super::assert_gpu_wgsl_valid;

    /// GPU math-intrinsic scalar width.
    /// A math intrinsic (sqrt) inside an f32 kernel must produce an f32 result,
    /// not an f64 result that gets width-cast before storing into the f32 buffer.
    /// This test checks both WGSL validity (no f64/f32 mismatch) and value correctness.
    #[test]
    fn sqrt_f32_buffer_width_correct() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array
use system.math

fn main()
    gpu let a = [1.0, 4.0, 9.0, 16.0]
    gpu var dst = [0.0, 0.0, 0.0, 0.0]
    gpu forall i in 0..4
        dst[i] = sqrt(a[i])
",
        );
        // Value correctness: sqrt of perfect squares.
        assert_gpu_runs_with_output(
            "
use system.gpu
use system.collections.array
use system.math
use system.io

fn main()
    gpu let a = [1.0, 4.0, 9.0, 16.0]
    gpu var dst = [0.0, 0.0, 0.0, 0.0]
    gpu forall i in 0..4
        dst[i] = sqrt(a[i])
    let result = dst
    println(f'{result[0]} {result[1]} {result[2]} {result[3]}')
",
            "1.0 2.0 3.0 4.0",
        );
    }

    /// Math intrinsic with f64 buffers (high-precision floats that don't
    /// round-trip in f32).
    #[test]
    fn sqrt_f64_buffer_width_correct() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array
use system.math

fn main()
    gpu let a = [1.4142135623730951, 3.7416573867739413, 5.744562646538029, 7.745966692414834]
    gpu var dst = [1.4142135623730951, 3.7416573867739413, 5.744562646538029, 7.745966692414834]
    gpu forall i in 0..4
        dst[i] = sqrt(a[i])
",
        );
    }

    /// Another math intrinsic: sin(x) on f32 buffers.
    #[test]
    fn sin_f32_buffer_width_correct() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array
use system.math

fn main()
    gpu let a = [0.0, 1.0, 2.0, 3.0]
    gpu var dst = [0.0, 0.0, 0.0, 0.0]
    gpu forall i in 0..4
        dst[i] = sin(a[i])
",
        );
    }
}

mod multi_if {
    use super::super::helpers::compile_to_wgsl;
    use super::assert_gpu_wgsl_valid;

    /// Sequential if chains with scope handling.
    /// Tests that the structurizer properly handles local variable declarations
    /// across sequential if statements. This exercises the case where multiple
    /// sequential ifs need to see the same function-scope vars.
    #[test]
    fn sequential_ifs_with_local_accumulation_emit_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [10, 20, 30, 40]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var sum = 0
        if a[i] > 0
            sum = sum + a[i]
        if b[i] > 15
            sum = sum + b[i]
        dst[i] = sum
",
        );
    }

    /// Nested if statements with local vars. This ensures the structurizer
    /// does not try to redeclare locals inside nested if bodies.
    #[test]
    fn nested_ifs_with_local_vars_emit_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [10, 20, 30, 40]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var sum = 0
        var count = 0
        if a[i] > 1
            sum = sum + a[i]
            count = count + 1
            if b[i] < 30
                sum = sum + b[i]
                count = count + 1
        dst[i] = sum
",
        );
    }

    /// If-else statement inside a kernel body. Tests that the structurizer
    /// properly emits the else clause when needed.
    #[test]
    fn if_else_with_accumulation_emit_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var result = 0
        if a[i] > 2
            result = 1
        else
            result = 0
        dst[i] = result
",
        );
    }

    /// Plain if (no else) with trailing code. The trailing code must NOT
    /// be nested inside the if block. This is the core bug test: it proves
    /// the bug exists by checking WGSL structure.
    #[test]
    fn plain_if_with_trailing_code_emits_naga_valid_wgsl() {
        let wgsl = compile_to_wgsl(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var sum = 0
        if a[i] > 2
            sum = sum + 100
        dst[i] = sum + a[i]
",
        );

        // First: naga must validate (smoke test).
        let module = naga::front::wgsl::parse_str(&wgsl).unwrap_or_else(|err| {
            panic!(
                "naga parse failed: {}\nWGSL:\n{}",
                err.emit_to_string(&wgsl),
                wgsl
            )
        });
        let mut validator = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        );
        validator
            .validate(&module)
            .unwrap_or_else(|err| panic!("naga validate failed: {:?}\nWGSL:\n{}", err, wgsl));

        // CORE: the trailing `dst[i] = ...` store must land AFTER the inner
        // if's closing brace, i.e. at function scope, not nested inside the if.
        // Find the SECOND if statement (the inner data-conditional, skipping the outer
        // bounds-check if), find its matching closing brace, then assert the trailing
        // store appears after that brace at the SAME indentation level (i.e. right after
        // the else clause closes).
        let mut if_positions = vec![];
        let mut search_start = 0;
        while let Some(pos) = wgsl[search_start..].find("if (") {
            if_positions.push(search_start + pos);
            search_start = search_start + pos + 1;
        }
        let if_start = if_positions
            .get(1)
            .copied()
            .expect("should have at least two if statements (bounds-check and inner condition)");

        // Balance braces starting from the opening brace after "if ("
        let mut brace_depth = 0;
        let mut if_end = if_start;
        let mut found_open = false;
        for (i, ch) in wgsl[if_start..].char_indices() {
            if ch == '{' {
                found_open = true;
                brace_depth += 1;
            } else if ch == '}' {
                brace_depth -= 1;
                if found_open && brace_depth == 0 {
                    if_end = if_start + i;
                    break;
                }
            }
        }

        // Find the dst assignment position (could be multiple; we want the one after
        // the inner if closes).
        let dst_assign = wgsl[if_end..]
            .find("dst[i32")
            .expect("should have dst[i32] assignment after inner if");
        let dst_assign = if_end + dst_assign;

        assert!(
            dst_assign > if_end,
            "WGSL structure bug: dst[i32] assignment at pos {} should be AFTER the inner if block's closing brace at pos {} (not nested inside). WGSL:\n{}",
            dst_assign, if_end, wgsl
        );
    }

    /// Sequential ifs with trailing code. Both ifs should close before
    /// the final store. This is a smoke test verifying naga accepts the
    /// complex control flow structure.
    #[test]
    fn sequential_ifs_with_trailing_code_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var sum = 0
        if a[i] > 2
            sum = sum + 10
        if a[i] < 3
            sum = sum + 20
        dst[i] = sum
",
        );
    }

    /// Nested ifs with trailing code. This verifies that naga accepts
    /// the nested control flow.
    #[test]
    fn nested_ifs_with_trailing_code_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [10, 20, 30, 40]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var sum = 0
        if a[i] > 1
            sum = sum + a[i]
            if b[i] > 15
                sum = sum + 100
        dst[i] = sum
",
        );
    }

    /// If-else with trailing code. This verifies the if-else path
    /// alongside plain-if path.
    #[test]
    fn if_else_with_trailing_code_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        var sum = 0
        if a[i] > 2
            sum = sum + 10
        else
            sum = sum + 5
        dst[i] = sum + 1
",
        );
    }
}

mod gpu_for_2d {
    use super::super::utils::assert_compiler_error;
    use super::assert_gpu_wgsl_valid;

    /// Basic 2D gpu forall with literal bounds (4x3 grid).
    /// Emits WGSL with @workgroup_size(16, 16, 1), uses _local_id.y and _workgroup_id.y,
    /// and has the 2D bounds guard.
    #[test]
    fn gpu_for_2d_literal_bounds_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall x, y in 0..4, 0..3
        dst[y * 4 + x] = x + y
",
        );
    }

    /// 2D gpu forall with larger bounds (32x32 grid).
    #[test]
    fn gpu_for_2d_large_bounds_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall x, y in 0..32, 0..32
        dst[y * 32 + x] = x + y
",
        );
    }

    /// 2D gpu forall with runtime bounds compiles and emits valid WGSL.
    #[test]
    fn gpu_for_2d_runtime_bounds_emits_naga_valid_wgsl() {
        assert_gpu_wgsl_valid(
            "
use system.gpu
use system.collections.array

fn main()
    let w = 4
    let h = 3
    gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall x, y in 0..w, 0..h
        dst[y * 4 + x] = x + y
",
        );
    }

    /// Reject >2 loop variables.
    #[test]
    fn gpu_for_3d_is_rejected() {
        assert_compiler_error(
            "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall x, y, z in 0..2, 0..2, 0..2
        dst[z * 4 + y * 2 + x] = x + y + z
",
            "at most 2 loop variables",
        );
    }

    /// Reject 2 vars with a single range.
    #[test]
    fn gpu_for_2d_missing_second_range_is_rejected() {
        assert_compiler_error(
            "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = [0, 0, 0, 0]
    gpu forall x, y in 0..4
        dst[0] = 0
",
            "2D gpu forall requires two comma-separated ranges",
        );
    }
}

/// Browser-portability tests: ensure no i64 literals or i64 array types
/// in WGSL output, only i32 (WebGPU/Tint has no 64-bit int support).
mod browser_portability {
    use super::super::helpers::compile_to_wgsl;

    #[test]
    fn int_buffer_emits_i32_not_i64_in_wgsl() {
        // int buffers must compile to array<i32>, not array<i64>.
        let source = "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = a[i] * 2
";
        let wgsl = compile_to_wgsl(source);

        // Verify no i64 array declarations
        assert!(
            !wgsl.contains("array<i64>"),
            "WGSL should use array<i32> for int buffers, not array<i64>. Found in:\n{}",
            wgsl
        );

        // Verify no i64 type name
        assert!(
            !wgsl.contains("i64"),
            "WGSL should contain NO i64 type keyword. Found in:\n{}",
            wgsl
        );

        // Verify no li suffix (i64 literal marker in WGSL)
        assert!(
            !wgsl.contains("li"),
            "WGSL should contain NO 'li' suffix (i64 literal). Found in:\n{}",
            wgsl
        );

        // Verify i32 is present (for array and operations)
        assert!(
            wgsl.contains("array<i32>"),
            "WGSL should contain array<i32> for int buffers. Full WGSL:\n{}",
            wgsl
        );
    }

    #[test]
    fn int_buffer_div_emits_i32_operations_in_wgsl() {
        // Div/mod on int should operate on i32 (not i64) in WGSL.
        let source = "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [10, 20, 30, 40]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = a[i] / 2
";
        let wgsl = compile_to_wgsl(source);

        // No i64 anywhere
        assert!(
            !wgsl.contains("i64"),
            "Div on int buffer should not emit i64. Found in:\n{}",
            wgsl
        );

        // Array uses i32
        assert!(
            wgsl.contains("array<i32>"),
            "Div kernel should use array<i32>. Found in:\n{}",
            wgsl
        );
    }

    #[test]
    fn float_buffer_still_uses_f32_in_wgsl() {
        // f32 buffers (from floating-point literals) remain f32, unchanged.
        let source = "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu var dst = [0.0, 0.0, 0.0, 0.0]
    gpu forall i in 0..4
        dst[i] = a[i] + 1.0
";
        let wgsl = compile_to_wgsl(source);

        // Should use f32, not f64
        assert!(
            wgsl.contains("array<f32>"),
            "f32 buffer should use array<f32>. Found in:\n{}",
            wgsl
        );

        assert!(
            !wgsl.contains("array<f64>"),
            "f32 buffer should NOT use array<f64>. Found in:\n{}",
            wgsl
        );
    }
}

/// Value-correctness after browser portability: int buffers round-trip
/// through device with i32 WGSL and host-side marshalling.
mod int_marshalling {
    use super::super::device::assert_gpu_runs_with_output;

    #[test]
    fn small_int_values_round_trip_with_marshalling() {
        // Small values (< i32::MAX) should round-trip correctly.
        let source = "
use system.io
use system.gpu
use system.collections.array

gpu let vals = [0, 1, 100, 1000, 10000, 100000]
gpu var result = [0, 0, 0, 0, 0, 0]
gpu forall i in 0..6
    result[i] = vals[i]
let host = result
println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]} {host[5]}')
";
        assert_gpu_runs_with_output(source, "0 1 100 1000 10000 100000");
    }

    #[test]
    fn int_arithmetic_after_marshalling_is_correct() {
        // Arithmetic on marshalled values should be correct.
        let source = "
use system.io
use system.gpu
use system.collections.array

gpu let a = [10, 20, 30, 40]
gpu let b = [5, 6, 7, 8]
gpu var result = [0, 0, 0, 0]
gpu forall i in 0..4
    result[i] = a[i] + b[i]
let host = result
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
        assert_gpu_runs_with_output(source, "15 26 37 48");
    }
}
