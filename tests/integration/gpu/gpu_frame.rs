// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for `gpu frame` statement surface (MILESTONE 1 acceptance).
//!
//! M1 scope: parser accepts gpu frame syntax, type-checker validates empty body,
//! MIR lowering delegates to gpu_for lowering. Detailed ping-pong buffer validation
//! (exactly 1 read, exactly 1 write, disjoint) is M1b implementation.

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

// MILESTONE 1b: Ping-pong buffer validation tests

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

// MILESTONE 2: Value-verified frame kernel (Conway Game of Life, simplified)

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
