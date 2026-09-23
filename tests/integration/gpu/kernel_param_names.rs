// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A kernel parameter's source name must never decide what the WGSL backend
//! does with it.
//!
//! Two properties are pinned here. First, a captured value is pooled into the
//! scalar-capture uniform whatever it is called: the per-axis loop-bound and
//! range-start uniforms the compiler injects are recognised by a marker on the
//! parameter, so a user capture spelled like one of them (`_start`,
//! `_bound_x`, `_uniform_bound`) still carries its own value. Second, a source
//! name that WGSL reserves, or that shadows a name the emitter synthesizes
//! (`_inputs`, `_global_id`, ...), is renamed on the way into the shader so the
//! module still compiles and every buffer still binds to the right data.

use super::device::assert_gpu_runs_with_output;
use super::helpers::{assert_gpu_wgsl_valid, compile_to_wgsl};

const RUNTIME_START_CAPTURE: &str = "
use system.gpu

fn main()
    gpu var buf = [0, 0, 0, 0]
    let _start = 7
    gpu forall i in 0..4
        buf[i] = i + _start
    let host = buf
    println(f\"{host[3]}\")
";

const TOP_LEVEL_START_SCALE_CAPTURE: &str = "
const N = 4

gpu var dst = [0.0, 0.0, 0.0, 0.0]
let _start_scale f32 = 2.5
let other f32 = 1.0
forall i in 0..N
    dst[i] = _start_scale + other

let host = dst
println(f'{host[0]} {host[3]}')
";

/// The same capture inside `main`: the WGSL helper lowers function bodies, not
/// top-level script statements.
const START_SCALE_CAPTURE_IN_MAIN: &str = "
use system.gpu

fn main()
    gpu var dst = [0.0, 0.0, 0.0, 0.0]
    let _start_scale f32 = 2.5
    let other f32 = 1.0
    gpu forall i in 0..4
        dst[i] = _start_scale + other
";

const LOOP_CONTROL_NAMED_CAPTURES: &str = "
use system.gpu

fn main()
    gpu var buf = [0, 0, 0, 0]
    let _bound_x = 100
    let _uniform_bound = 20
    let n = 4
    gpu forall i in 0..n
        buf[i] = i + _bound_x + _uniform_bound
    let host = buf
    println(f'{host[0]} {host[3]}')
";

/// A scalar capture named `_start` is a captured value, not the range-start
/// uniform: the kernel must still be valid WGSL and read the value 7.
#[test]
fn capture_named_like_range_start_emits_valid_wgsl() {
    assert_gpu_wgsl_valid(RUNTIME_START_CAPTURE);
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn capture_named_like_range_start_reads_its_value() {
    assert_gpu_runs_with_output(RUNTIME_START_CAPTURE, "10");
}

/// An `f32` capture whose name begins with `_start` is pooled with the other
/// scalar captures rather than bound as a `u32` range start.
#[test]
fn capture_prefixed_like_range_start_emits_valid_wgsl() {
    assert_gpu_wgsl_valid(START_SCALE_CAPTURE_IN_MAIN);
}

/// The same capture in a top-level script, run on the device.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn top_level_capture_prefixed_like_range_start_reads_its_value() {
    assert_gpu_runs_with_output(TOP_LEVEL_START_SCALE_CAPTURE, "3.5 3.5");
}

/// Captures spelled like the loop-bound uniforms sit beside a real runtime
/// bound; each must keep its own value and the real bound must still apply.
#[test]
fn captures_named_like_loop_bounds_emit_valid_wgsl() {
    assert_gpu_wgsl_valid(LOOP_CONTROL_NAMED_CAPTURES);
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn captures_named_like_loop_bounds_read_their_values() {
    assert_gpu_runs_with_output(LOOP_CONTROL_NAMED_CAPTURES, "120 123");
}

const RESERVED_BUFFER_NAMES: &str = "
use system.gpu

fn main()
    gpu var filter = [0, 0, 0, 0]
    gpu var loop = [0, 0, 0, 0]
    gpu let target = [5, 6, 7, 8]
    gpu forall i in 0..4
        filter[i] = i
        loop[i] = target[i] * 2
    let a = filter
    let b = loop
    println(f'{a[3]} {b[0]} {b[3]}')
";

const RESERVED_SCALAR_CAPTURE: &str = "
use system.gpu

fn main()
    gpu var out = [0, 0, 0, 0]
    let filter = 40
    let module = 2
    gpu forall i in 0..4
        out[i] = filter + module + i
    let host = out
    println(f'{host[0]} {host[3]}')
";

const SYNTHETIC_NAMED_CAPTURES: &str = "
use system.gpu

fn main()
    gpu var _global_id = [0, 0, 0, 0]
    gpu let _inputs = [1, 2, 3, 4]
    let _Inputs = 10
    gpu forall i in 0..4
        _global_id[i] = _inputs[i] + _Inputs
    let host = _global_id
    println(f'{host[0]} {host[3]}')
";

const SCALAR_CAPTURE_NAMED_INPUTS: &str = "
use system.gpu

fn main()
    gpu var dst = [0, 0, 0, 0]
    let _inputs = 9
    gpu forall i in 0..4
        dst[i] = _inputs + i
    let host = dst
    println(f'{host[0]} {host[3]}')
";

/// Buffers named after WGSL reserved words (`filter`, `loop`, `target`) are
/// renamed in the shader, which then parses and validates.
#[test]
fn buffers_named_after_wgsl_reserved_words_emit_valid_wgsl() {
    assert_gpu_wgsl_valid(RESERVED_BUFFER_NAMES);
}

/// The renamed buffers still bind to their own data.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn buffers_named_after_wgsl_reserved_words_read_and_write_their_data() {
    assert_gpu_runs_with_output(RESERVED_BUFFER_NAMES, "3 10 16");
}

#[test]
fn scalar_captures_named_after_wgsl_reserved_words_emit_valid_wgsl() {
    assert_gpu_wgsl_valid(RESERVED_SCALAR_CAPTURE);
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn scalar_captures_named_after_wgsl_reserved_words_read_their_values() {
    assert_gpu_runs_with_output(RESERVED_SCALAR_CAPTURE, "42 45");
}

/// Buffers named exactly like emitter-synthesized identifiers (`_global_id`,
/// the builtin parameter; `_inputs`, the pooled scalar uniform) are renamed so
/// they cannot shadow or redeclare those names.
#[test]
fn buffers_named_like_synthesized_identifiers_emit_valid_wgsl() {
    assert_gpu_wgsl_valid(SYNTHETIC_NAMED_CAPTURES);
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn buffers_named_like_synthesized_identifiers_read_and_write_their_data() {
    assert_gpu_runs_with_output(SYNTHETIC_NAMED_CAPTURES, "11 14");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn scalar_capture_named_inputs_reads_its_value() {
    assert_gpu_runs_with_output(SCALAR_CAPTURE_NAMED_INPUTS, "9 12");
}

/// A renamed buffer never keeps its reserved spelling as a WGSL declaration.
#[test]
fn reserved_buffer_name_is_not_declared_verbatim() {
    let wgsl = compile_to_wgsl(RESERVED_BUFFER_NAMES);
    for reserved in ["filter", "loop", "target"] {
        assert!(
            !wgsl.contains(&format!("> {}:", reserved)),
            "buffer `{}` must be renamed in the shader, got:\n{}",
            reserved,
            wgsl
        );
    }
}

/// An `f32` buffer whose name merely contains `f16` (here `coef16`) must not
/// pull in the `enable f16;` directive: that needs the `SHADER_F16` feature,
/// which adapters without half-precision support refuse.
#[test]
fn identifier_containing_f16_does_not_enable_f16() {
    let wgsl = compile_to_wgsl(
        "
use system.gpu

fn main()
    gpu let coef16 = [1.5, 2.5, 3.5, 4.5]
    gpu var dst = [0.0, 0.0, 0.0, 0.0]
    gpu forall i in 0..4
        dst[i] = coef16[i] * 2.0
",
    );
    assert!(
        !wgsl.contains("enable f16;"),
        "an f32 kernel must not enable f16, got:\n{}",
        wgsl
    );
    assert!(
        wgsl.contains("coef16"),
        "the buffer keeps its source name, got:\n{}",
        wgsl
    );
}
