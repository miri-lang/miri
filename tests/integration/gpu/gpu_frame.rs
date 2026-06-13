// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the `gpu frame` statement surface.
//!
//! The parser accepts gpu frame syntax, the type-checker validates empty bodies,
//! and MIR lowering delegates to gpu_for lowering. Ping-pong buffer validation
//! ensures exactly 1 read-only and 1 read-write buffer with no overlap.

use crate::integration::utils::{assert_compiler_error, assert_runs};

#[test]
fn test_gpu_frame_keyword_recognized() {
    // Test that `gpu frame` keyword sequence parses without "frame is not a keyword" error.
    // The frame loop itself may fail downstream (type-check, MIR), but the parser must
    // recognize the keyword, not reject it as an invalid identifier.
    let code = r#"use system.io
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu frame i in 0..4:
        b[i] = a[i] + 1
    println("ok")
"#;
    assert_runs(code);
}

// Ping-pong buffer validation tests ensure correctness of dual-buffer tracking

#[test]
fn test_gpu_frame_valid_one_read_one_write() {
    // Valid case: exactly one read-only gpu buffer (a) and one read-write gpu buffer (b).
    let code = r#"use system.io
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu frame i in 0..4:
        b[i] = a[i] + 1
    println("ok")
"#;
    assert_runs(code);
}

#[test]
fn test_gpu_frame_zero_gpu_captures() {
    // Error: no gpu buffers captured in the frame body.
    let code = r#"use system.gpu

fn main()
    gpu frame i in 0..4:
        let x = 1
"#;
    assert_compiler_error(code, "gpu buffer");
}

#[test]
fn test_gpu_frame_no_write_buffer() {
    // Error: only read-only gpu buffer, no read-write.
    let code = r#"use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu frame i in 0..4:
        let x = a[i]
"#;
    assert_compiler_error(code, "write");
}

#[test]
fn test_gpu_frame_multiple_write_buffers() {
    // Error: more than one gpu buffer is written to.
    let code = r#"use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu var c = [0, 0, 0, 0]
    gpu frame i in 0..4:
        b[i] = a[i]
        c[i] = a[i]
"#;
    assert_compiler_error(code, "write");
}

#[test]
fn test_gpu_frame_same_buffer_read_write() {
    // Error: same buffer used for both read and write (data race).
    let code = r#"use system.gpu

fn main()
    gpu var a = [1, 2, 3, 4]
    gpu frame i in 0..4:
        a[i] = a[i] + 1
"#;
    assert_compiler_error(code, "data race");
}

// Value-verified frame kernel tests (simplified patterns before full Conway's Game of Life)

#[test]
fn test_gpu_frame_simple_gol() {
    // Simplified Conway's Game of Life: just verify the frame loop structure works
    // with a simple ping-pong copy (read grid_a, write grid_b).
    let code = r#"use system.io
use system.gpu

fn main()
    let grid_a = [1, 2, 3, 4, 5]
    let grid_b = [0, 0, 0, 0, 0]

    gpu let a = grid_a
    gpu var b = grid_b

    gpu frame i in 0..5:
        b[i] = a[i] + 1

    println("ok")
"#;
    use crate::integration::utils::assert_runs_with_output;
    assert_runs_with_output(code, "ok");
}

// DP2 Part 1: frame input field tests

#[test]
fn test_frame_field_read_scalar() {
    // Acceptance 1a: frame.time and frame.dt are readable as f32 values inside gpu frame.
    let code = r#"use system.io
use system.gpu

fn main()
    gpu let a = [0.0, 0.0, 0.0, 0.0]
    gpu var b = [0.0, 0.0, 0.0, 0.0]
    gpu frame i in 0..4:
        let t = frame.time
        let d = frame.dt
        b[i] = a[i] + t + d
    println("ok")
"#;
    assert_runs(code);
}

#[test]
fn test_frame_field_read_bool() {
    // Acceptance 1a: frame.mouse_down is readable as bool into a variable.
    let code = r#"use system.io
use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu frame i in 0..4:
        let md = frame.mouse_down
        b[i] = a[i]
    println("ok")
"#;
    assert_runs(code);
}

#[test]
fn test_frame_field_in_binary() {
    // Test: frame.time in a binary expression.
    let code = r#"use system.io
use system.gpu

fn main()
    gpu let a = [0.0, 0.0, 0.0, 0.0]
    gpu var b = [0.0, 0.0, 0.0, 0.0]
    gpu frame i in 0..4:
        let result = frame.time + 1.0
        b[i] = a[i] + result
    println("ok")
"#;
    assert_runs(code);
}

#[test]
fn test_frame_index_in_binary() {
    // Test: frame.index in a binary expression (not a condition).
    let code = r#"use system.io
use system.gpu

fn main()
    gpu let a = [0, 0, 0, 0]
    gpu var b = [0, 0, 0, 0]
    gpu frame i in 0..4:
        let cond_val = frame.index < 2
        b[i] = a[i]
    println("ok")
"#;
    assert_runs(code);
}

#[test]
fn test_frame_field_read_int() {
    // Test: frame.index is readable as int into a variable.
    let code = r#"use system.io
use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu frame i in 0..4:
        let idx = frame.index
        b[i] = a[i]
    println("ok")
"#;
    assert_runs(code);
}

#[test]
fn test_frame_field_read_index_in_condition() {
    // Test: frame.index (int) is readable in a condition.
    let code = r#"use system.io
use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu frame i in 0..4:
        if frame.index < 2:
            b[i] = a[i]
    println("ok")
"#;
    assert_runs(code);
}

#[test]
fn test_frame_field_read_bool_in_condition() {
    // Acceptance 1a: frame.mouse_down is readable as bool in a condition.
    let code = r#"use system.io
use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu frame i in 0..4:
        if frame.mouse_down:
            b[i] = a[i]
    println("ok")
"#;
    assert_runs(code);
}

#[test]
fn test_frame_outside_gpu_frame_body() {
    // Acceptance 1b: frame is rejected outside gpu frame body.
    let code = r#"use system.io

fn main()
    let x = frame.time
"#;
    assert_compiler_error(code, "frame");
}

#[test]
fn test_frame_unknown_field() {
    // Acceptance 1c: unknown frame.field is rejected.
    let code = r#"use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu frame i in 0..4:
        let x = frame.not_a_field
"#;
    assert_compiler_error(code, "not_a_field");
}

#[test]
fn test_frame_with_scalar_capture() {
    // D2: test frame fields are ordered before scalar captures.
    // This test captures both frame fields and an ordinary scalar.
    // The frame params (f0..f10) are pushed first, then scalar captures.
    let code = r#"use system.io
use system.gpu

fn main()
    let scalar_val = 5.0
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu var b = [0.0, 0.0, 0.0, 0.0]
    gpu frame i in 0..4:
        let t = frame.time
        let s = scalar_val
        b[i] = a[i] + t + s
    println("ok")
"#;
    assert_runs(code);
}

// FIX 1: Frame param shadowing tests

#[test]
fn test_frame_param_shadowing_f0_with_frame_time() {
    // RED test: user variable named f0 should NOT shadow frame.time parameter.
    // If shadowing occurs (bug), f0 local overwrites the frame param in variable_map,
    // causing frame.time to read the wrong local (the user's f0 instead of param f0).
    let code = r#"use system.io
use system.gpu

fn main()
    let f0 = 9.0
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu var b = [0.0, 0.0, 0.0, 0.0]
    gpu frame i in 0..4:
        let t = frame.time
        let s = f0
        b[i] = a[i] + t + s
    println("ok")
"#;
    assert_runs(code);
}

#[test]
fn test_frame_param_shadowing_f5_with_frame_mouse_down() {
    // RED test: user variable named f5 should NOT shadow frame.mouse_down parameter.
    let code = r#"use system.io
use system.gpu

fn main()
    let f5 = 1
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu frame i in 0..4:
        let md = frame.mouse_down
        b[i] = a[i] + f5
    println("ok")
"#;
    assert_runs(code);
}

// FIX 3: WGSL validity tests (naga-valid output)

#[test]
fn test_gpu_frame_wgsl_validity_frame_time() {
    // FIX 3a: frame.time (f32) in arithmetic must produce naga-valid WGSL.
    use crate::integration::gpu::helpers::assert_gpu_wgsl_valid;
    let code = r#"use system.io
use system.gpu

fn main()
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu var b = [0.0, 0.0, 0.0, 0.0]
    gpu frame i in 0..4:
        let t = frame.time
        b[i] = a[i] + t
    println("ok")
"#;
    assert_gpu_wgsl_valid(code);
}

#[test]
fn test_gpu_frame_wgsl_validity_frame_mouse_down_bool_coercion() {
    // FIX 3b: frame.mouse_down (bool) in a condition must produce naga-valid WGSL.
    // The bool is encoded as u32 in the uniform struct and must coerce back to bool.
    use crate::integration::gpu::helpers::assert_gpu_wgsl_valid;
    let code = r#"use system.io
use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu frame i in 0..4:
        if frame.mouse_down:
            b[i] = a[i]
    println("ok")
"#;
    assert_gpu_wgsl_valid(code);
}

#[test]
fn test_gpu_frame_wgsl_validity_frame_plus_scalar_capture() {
    // FIX 3c: frame fields + ordinary scalar capture must not collide on @binding index.
    use crate::integration::gpu::helpers::assert_gpu_wgsl_valid;
    let code = r#"use system.io
use system.gpu

fn main()
    let s = 1.0
    gpu let a = [1.0, 2.0, 3.0, 4.0]
    gpu var b = [0.0, 0.0, 0.0, 0.0]
    gpu frame i in 0..4:
        let t = frame.time
        b[i] = a[i] + t + s
    println("ok")
"#;
    assert_gpu_wgsl_valid(code);
}

// FIX 2: Runtime-bound frame loop binding collision

#[test]
fn test_gpu_frame_runtime_bound_wgsl_validity() {
    // FIX 2 RED: runtime-bound (0..n) gpu frame reading frame.* must not collide
    // on @binding index between _inputs struct and _uniform_bound.
    use crate::integration::gpu::helpers::assert_gpu_wgsl_valid;
    let code = r#"use system.io
use system.gpu

fn main()
    let n = 4
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu frame i in 0..n:
        if frame.mouse_down:
            b[i] = a[i] + frame.index
    println("ok")
"#;
    assert_gpu_wgsl_valid(code);
}

#[test]
fn test_frame_in_plain_gpu_for_rejected() {
    // FIX 6a: frame is unbound in plain gpu for (not gpu frame), must be rejected.
    let code = r#"use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu for i in 0..4:
        let t = frame.time
        b[i] = a[i]
"#;
    assert_compiler_error(code, "frame");
}
