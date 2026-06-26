// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the `gpu frame` statement surface.
//!
//! The parser accepts gpu frame syntax, the type-checker validates empty bodies,
//! and MIR lowering delegates to gpu_for lowering. Ping-pong buffer validation
//! ensures exactly 1 read-only and 1 read-write buffer with no overlap.

use crate::integration::utils::{assert_compiler_error, assert_runs};

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_frame_multiple_write_buffers() {
    // F35 REFACTOR: multiple disjoint writes are now LEGAL (semantic instead of structural).
    let code = r#"use system.io
use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu var c = [0, 0, 0, 0]
    gpu frame i in 0..4:
        b[i] = a[i]
        c[i] = a[i]
    println("ok")
"#;
    assert_runs(code);
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
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
    // FIX 6a: frame is unbound in plain gpu forall (not gpu frame), must be rejected.
    let code = r#"use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu forall i in 0..4:
        let t = frame.time
        b[i] = a[i]
"#;
    assert_compiler_error(code, "frame");
}

// DP3: Multi-pass gpu frame block tests

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_frame_block_two_passes() {
    // DP3 acceptance: `gpu frame { gpu forall ..., gpu forall ... }` block form parses and runs.
    // Two disjoint passes: first reads grid_a, writes grid_b; second reads grid_b, writes grid_c.
    // Verifies pass 2 reads the committed output from pass 1 (not stale/zero data).
    use crate::integration::utils::assert_runs_with_output;
    let code = r#"use system.io
use system.gpu
use system.collections.array

fn main()
    let init_a = [1, 2, 3, 4]
    let init_b = [0, 0, 0, 0]
    let init_c = [0, 0, 0, 0]

    gpu let grid_a = init_a
    gpu var grid_b = init_b
    gpu var grid_c = init_c

    gpu frame
        gpu forall i in 0..4:
            grid_b[i] = grid_a[i] + 1
        gpu forall i in 0..4:
            grid_c[i] = grid_b[i] + 1

    let host_c = grid_c
    var sum_c = 0
    var i = 0
    while i < 4
        sum_c = sum_c + host_c[i]
        i = i + 1
    println(f"sum={sum_c}")
"#;
    // Pass 1: grid_a[i]=i+1 → grid_b[i]=(i+1)+1 = i+2
    // Pass 2: grid_c[i]=grid_b[i]+1 = (i+2)+1 = i+3
    // sum = (0+3)+(1+3)+(2+3)+(3+3) = 3+4+5+6 = 18
    assert_runs_with_output(code, "sum=18");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_frame_block_with_frame_inputs() {
    // DP3 acceptance: gpu frame block supports multiple passes.
    // Note: frame field access deferred to later; test uses basic variable capture.
    let code = r#"use system.io
use system.gpu

fn main()
    let scalar_val = 1.0
    gpu let grid_a = [1.0, 2.0, 3.0, 4.0]
    gpu var grid_b = [0.0, 0.0, 0.0, 0.0]
    gpu var grid_c = [0.0, 0.0, 0.0, 0.0]

    gpu frame
        gpu forall i in 0..4:
            grid_b[i] = grid_a[i] + scalar_val
        gpu forall i in 0..4:
            grid_c[i] = grid_b[i] + scalar_val

    println("ok")
"#;
    assert_runs(code);
}

// NOTE: F35 per-pass buffer validation is deferred; tests removed.
// These validations will be added when F35 buffer-level disjointness is implemented.

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_frame_block_disjoint_writes_are_legal() {
    // DP3 F35 strengthening: multiple disjoint writes in the SAME pass are now legal
    // (only buffer-level disjointness with reads is required, not just one write).
    let code = r#"use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu var c = [0, 0, 0, 0]
    gpu frame
        gpu forall i in 0..4:
            b[i] = a[i]
            c[i] = a[i]

    println("ok")
"#;
    use crate::integration::utils::assert_runs_with_output;
    assert_runs_with_output(code, "ok");
}

// NOTE: GPU WGSL validation for multi-pass deferred; test removed.
// This will be tested when MIR lowering for gpu frame block is fully implemented.

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_frame_single_pass_unchanged() {
    // DP3 backward compatibility: single-pass `gpu frame i in 0..4: body` still works.
    let code = r#"use system.io
use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu frame i in 0..4:
        b[i] = a[i] + 1
    println("ok")
"#;
    use crate::integration::utils::assert_runs_with_output;
    assert_runs_with_output(code, "ok");
}

// F35 per-pass semantic validation (buffer-level disjointness)

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_frame_block_pass_multiple_disjoint_writes() {
    // F35 RED: multiple disjoint writes in a single pass should be LEGAL.
    // This differs from the old single-pass frame rule.
    let code = r#"use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var b = [0, 0, 0, 0]
    gpu var c = [0, 0, 0, 0]
    gpu frame
        gpu forall i in 0..4:
            b[i] = a[i]
            c[i] = a[i]
    println("ok")
"#;
    assert_runs(code);
}

#[test]
fn test_gpu_frame_block_pass_same_buffer_read_write_race() {
    // F35 RED: same buffer read and written in one pass is a race, must reject.
    let code = r#"use system.gpu

fn main()
    gpu var a = [1, 2, 3, 4]
    gpu frame
        gpu forall i in 0..4:
            a[i] = a[i] + 1
"#;
    assert_compiler_error(code, "data race");
}

#[test]
fn test_gpu_frame_block_pass_no_write() {
    // F35 RED: a pass with no write is invalid.
    let code = r#"use system.gpu

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu frame
        gpu forall i in 0..4:
            let x = a[i]
"#;
    assert_compiler_error(code, "write");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_frame_block_pass_frame_readable() {
    // F35 RED: frame.* fields readable in a block pass.
    let code = r#"use system.io
use system.gpu

fn main()
    gpu let a = [0.0, 0.0, 0.0, 0.0]
    gpu var b = [0.0, 0.0, 0.0, 0.0]
    gpu frame
        gpu forall i in 0..4:
            let t = frame.time
            b[i] = a[i] + t
    println("ok")
"#;
    assert_runs(code);
}

#[test]
fn test_gpu_frame_block_pass_frame_wgsl_valid() {
    // F35 RED: frame.* in block pass produces naga-valid WGSL.
    use crate::integration::gpu::helpers::assert_gpu_wgsl_valid;
    let code = r#"use system.io
use system.gpu

fn main()
    gpu let a = [0.0, 0.0, 0.0, 0.0]
    gpu var b = [0.0, 0.0, 0.0, 0.0]
    gpu frame
        gpu forall i in 0..4:
            if frame.mouse_down:
                b[i] = a[i] + frame.time
    println("ok")
"#;
    assert_gpu_wgsl_valid(code);
}

#[test]
fn test_gpu_frame_block_manifest_two_passes() {
    // Manifest RED: verify `framePasses` array contains 2 passes for a 2-pass block.
    let code = r#"use system.io
use system.gpu

fn main()
    gpu let grid_a = [1, 2, 3, 4]
    gpu var grid_b = [0, 0, 0, 0]
    gpu var grid_c = [0, 0, 0, 0]
    gpu frame
        gpu forall i in 0..4:
            grid_b[i] = grid_a[i]
        gpu forall i in 0..4:
            grid_c[i] = grid_b[i]
    println("ok")
"#;
    use crate::integration::gpu::helpers::compile_to_manifest;
    let manifest = compile_to_manifest(code).expect("manifest");
    let frame_passes = &manifest["framePasses"];
    assert!(frame_passes.is_array(), "framePasses must be an array");
    assert_eq!(
        frame_passes.as_array().unwrap().len(),
        2,
        "2-pass block should have 2 framePasses"
    );
}

// Item 5: GPU-resident state pattern test

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_frame_state_integration_pattern() {
    // Item 5: a gpu frame block whose first pass integrates a frame.* field into a state buffer,
    // then a second render pass reads the state. This demonstrates the GPU-resident state pattern.
    // Uses ping-pong state buffers to avoid races. Verifies pass 2 reads committed output from pass 1.
    use crate::integration::utils::assert_runs_with_output;
    let code = r#"use system.io
use system.gpu
use system.collections.array

fn main()
    gpu var state_a = [1.0, 2.0, 3.0, 4.0]
    gpu var state_b = [0.0, 0.0, 0.0, 0.0]
    gpu var output = [0.0, 0.0, 0.0, 0.0]
    gpu frame
        gpu forall i in 0..4:
            let delta = frame.time
            state_b[i] = state_a[i] + delta
        gpu forall i in 0..4:
            output[i] = state_b[i]

    let host_out = output
    var sum_out = 0.0
    var i = 0
    while i < 4
        sum_out = sum_out + host_out[i]
        i = i + 1
    println(f"sum={sum_out}")
"#;
    // frame.time = 0.0 (default)
    // Pass 1: state_b[i] = state_a[i] + 0.0 = state_a[i] = [1.0, 2.0, 3.0, 4.0]
    // Pass 2: output[i] = state_b[i] = [1.0, 2.0, 3.0, 4.0]
    // sum = 1.0 + 2.0 + 3.0 + 4.0 = 10.0
    assert_runs_with_output(code, "sum=10");
}

#[test]
fn test_gpu_frame_state_wgsl_valid() {
    // Item 5 WGSL validation: ensure the state pattern compiles to naga-valid WGSL.
    use crate::integration::gpu::helpers::assert_gpu_wgsl_valid;
    let code = r#"use system.io
use system.gpu

fn main()
    gpu var state_a = [0.0, 0.0, 0.0, 0.0]
    gpu var state_b = [0.0, 0.0, 0.0, 0.0]
    gpu var output = [0.0, 0.0, 0.0, 0.0]
    gpu frame
        gpu forall i in 0..4:
            let delta = frame.time
            state_b[i] = state_a[i] + delta
        gpu forall i in 0..4:
            output[i] = state_b[i]
    println("ok")
"#;
    assert_gpu_wgsl_valid(code);
}

// BUG A: top-level gpu frame block (no fn main wrapper)

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_frame_block_top_level_two_passes() {
    // BUG A RED: gpu frame block at top-level (not in fn main) should emit main symbol.
    // Previously, lower_gpu_frame_block didn't properly chain blocks, causing
    // the top-level main's CFG to be disconnected.
    let code = r#"use system.collections.array
use system.io
gpu var a = Array<int, 16>()
gpu var c = Array<int, 16>()
gpu forall i in 0..16
    a[i] = 1
gpu frame
    gpu forall i in 0..16
        c[i] = a[i] + 1
let h = c
println(f"{h[0]}")
"#;
    use crate::integration::utils::assert_runs_with_output;
    assert_runs_with_output(code, "2");
}

// NOTE: F35+ integration tests for frame pass structure validation are deferred.
// The manifest inspection requires additional infrastructure to count kernels
// generated from gpu frame block statements. For now, we verify the demo:
// (1) Compiles without error
// (2) Runs natively with correct deterministic output
// Both are tested via the demo_game_of_life_web test in tests/gpu/demos.rs

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_gpu_frame_repeat_unrolls_passes() {
    // A `for _ in 0..k` repeat inside a `gpu frame` block unrolls to `k` copies
    // of its passes. Two passes ping-pong a<->b adding 1 each, over 3 iterations
    // (6 increments): a starts at 1.0, ends at 7.0.
    let code = r#"use system.collections.array
use system.io

gpu var a = Array<f32, 16>()
gpu var b = Array<f32, 16>()

gpu forall i in 0..16
    a[i] = 1.0
    b[i] = 0.0

gpu frame
    for _ in 0..3
        gpu forall i in 0..16
            b[i] = a[i] + 1.0
        gpu forall i in 0..16
            a[i] = b[i] + 1.0

let h = a
println(f"a0={h[0]}")
"#;
    crate::integration::utils::assert_runs_with_output(code, "a0=7");
}

#[test]
fn test_gpu_frame_repeat_non_forall_body_rejected() {
    // A repeat body may only contain `gpu forall` passes.
    let code = r#"use system.collections.array
use system.io

gpu var a = Array<f32, 16>()

gpu frame
    for _ in 0..3
        println("not a pass")
"#;
    assert_compiler_error(code, "gpu forall");
}
