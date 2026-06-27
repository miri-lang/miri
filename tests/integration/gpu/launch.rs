// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Native `forall` dispatch tests. These exercise the full compiler-driven
// pipeline: MIR `TerminatorKind::GpuLaunch` → Cranelift translation →
// marshal captures → wgpu dispatch via `miri_gpu_launch_inline` →
// readback.
//
// Owns end-to-end value correctness for `forall` kernels: the WGSL
// scalar mapping aligns host and device widths (`int` → `i64`, `float`
// → `f64`) so reads/writes round-trip through device memory cleanly.

use super::device::assert_gpu_runs_with_output;
use super::utils::*;

/// Smoke test verifying the infrastructure layer is wired end-to-end:
/// compilation succeeds, the binary links against `libmiri_runtime_gpu.a`,
/// and the dispatch call into `miri_gpu_launch_inline` returns.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_for_dispatch_does_not_crash() {
    assert_runs(
        "
use system.gpu
use system.collections.array

gpu let a = [1, 2, 3, 4]
gpu let b = [10, 20, 30, 40]
gpu var dst = [0, 0, 0, 0]
gpu forall i in 0..4
    dst[i] = a[i] + b[i]
println(\"dispatched\")
",
    );
}

/// End-to-end test: `gpu let` / `gpu var` / `forall` / cross-residency
/// readback compiles and dispatches with ZERO `use` lines. Verifies that
/// implicit imports work on the GPU path — `println`, array literals, and
/// the `Accelerable` trait resolve without explicit imports.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn definition_of_done_program_compiles_with_zero_use_lines() {
    let source = "
gpu let a = [1.0, 2.0, 3.0, 4.0]
gpu let b = [5.0, 6.0, 7.0, 8.0]
gpu var dst = [0.0, 0.0, 0.0, 0.0]

gpu forall i in 0..4
    dst[i] = a[i] + b[i]

let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "6.0 8.0 10.0 12.0");
}

/// End-to-end value-correctness check for `int` (host i64 / WGSL i64):
/// element-wise add of two captured arrays into a writable destination.
/// Reads back the host buffer and prints it to compare against the
/// expected layout.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vector_add_int_round_trips_through_device() {
    let source = "
use system.gpu
use system.collections.array

gpu let a = [1, 2, 3, 4]
gpu let b = [10, 20, 30, 40]
gpu var dst = [0, 0, 0, 0]
gpu forall i in 0..4
    dst[i] = a[i] + b[i]
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "11 22 33 44");
}

/// End-to-end value-correctness check for scalar multiply: every element
/// of the captured `src` array is multiplied by a literal constant and
/// written to `dst`.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn scalar_multiply_int_round_trips_through_device() {
    let source = "
use system.gpu
use system.collections.array

gpu let src = [1, 2, 3, 4, 5, 6, 7, 8]
gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0]
gpu forall i in 0..8
    dst[i] = src[i] * 7
let host = dst
println(f'{host[0]} {host[7]}')
";
    assert_gpu_runs_with_output(source, "7 56");
}

/// End-to-end value-correctness check for element-wise multiply-add:
/// `dst[i] = a[i] * b[i] + c[i]` for all elements.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn elementwise_madd_round_trips_through_device() {
    let source = "
use system.gpu
use system.collections.array

gpu let a = [1, 2, 3, 4]
gpu let b = [10, 20, 30, 40]
gpu let c = [100, 200, 300, 400]
gpu var dst = [0, 0, 0, 0]
gpu forall i in 0..4
    dst[i] = a[i] * b[i] + c[i]
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    // Expected values: 1*10+100=110, 2*20+200=240, 3*30+300=390, 4*40+400=560
    assert_gpu_runs_with_output(source, "110 240 390 560");
}

/// End-to-end value-correctness check for bounds checking: a kernel with
/// iteration range `0..7` against an 8-element array. Threads 7..255 must
/// hit the synthesized bounds guard and skip the body. The last element
/// is initialized to 999 (sentinel); if the bounds guard is missing,
/// thread 7 would overwrite it.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn bounds_check_preserves_sentinel_past_range_end() {
    let source = "
use system.gpu
use system.collections.array

gpu var dst = [999, 999, 999, 999, 999, 999, 999, 999]
gpu forall i in 0..7
    dst[i] = i + 100
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]} {host[5]} {host[6]} {host[7]}')
";
    assert_gpu_runs_with_output(source, "100 101 102 103 104 105 106 999");
}

/// End-to-end value-correctness check for a fixed-size reduction: a
/// single-thread kernel (`0..1`) that computes the sum of all elements
/// in a captured array.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn reduction_fixed_sum_writes_single_total() {
    let source = "
use system.gpu
use system.collections.array

gpu let a = [1, 2, 3, 4, 5, 6, 7, 8]
gpu var dst = [0, 0, 0, 0]
gpu forall i in 0..1
    dst[0] = a[0] + a[1] + a[2] + a[3] + a[4] + a[5] + a[6] + a[7]
let host = dst
println(f'{host[0]}')
";
    assert_gpu_runs_with_output(source, "36");
}

/// Inner while-loop accumulation: each GPU thread sums the first two
/// elements of the array, accumulating into a local variable over
/// loop iterations. Expected: 1+2 = 3 for all threads.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn while_loop_value_round_trips_through_device() {
    let source = "
use system.gpu
use system.collections.array

gpu let a = [1, 2, 3, 4]
gpu var dst = [0, 0, 0, 0]
gpu forall i in 0..4
    var s = 0
    var j = 0
    while j < 2
        s = s + a[j]
        j = j + 1
    dst[i] = s
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "3 3 3 3");
}

/// Inner for-loop with continue: each thread sums array elements,
/// skipping index 2. Chunk 0: a[0]+a[1]+a[3]=1+2+4=7;
/// Chunk 1: a[4]+a[5]+a[7]=5+6+8=19.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn inner_loop_continue_value_round_trips_through_device() {
    let source = "
use system.gpu
use system.collections.array

gpu let a = [1, 2, 3, 4, 5, 6, 7, 8]
gpu var dst = [0, 0]
gpu forall i in 0..2
    var s = 0
    for j in 0..4
        if j == 2
            continue
        s = s + a[i * 4 + j]
    dst[i] = s
let host = dst
println(f'{host[0]} {host[1]}')
";
    assert_gpu_runs_with_output(source, "7 19");
}

/// Inner for-loop with break: each thread sums array elements until
/// index 2, then breaks. Chunk 0: a[0]+a[1]=1+2=3;
/// Chunk 1: a[4]+a[5]=5+6=11.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn inner_loop_break_value_round_trips_through_device() {
    let source = "
use system.gpu
use system.collections.array

gpu let a = [1, 2, 3, 4, 5, 6, 7, 8]
gpu var dst = [0, 0]
gpu forall i in 0..2
    var s = 0
    for j in 0..4
        if j == 2
            break
        s = s + a[i * 4 + j]
    dst[i] = s
let host = dst
println(f'{host[0]} {host[1]}')
";
    assert_gpu_runs_with_output(source, "3 11");
}

/// Triple-nested loops: each thread sums a 2x3 chunk.
/// Chunk 0: sum a[0..6]=1+2+3+4+5+6=21;
/// Chunk 1: sum a[6..12]=7+8+9+10+11+12=57.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn nested_loop_value_round_trips_through_device() {
    let source = "
use system.gpu
use system.collections.array

gpu let a = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]
gpu var dst = [0, 0]
gpu forall i in 0..2
    var s = 0
    for j in 0..2
        for k in 0..3
            s = s + a[i * 6 + j * 3 + k]
    dst[i] = s
let host = dst
println(f'{host[0]} {host[1]}')
";
    assert_gpu_runs_with_output(source, "21 57");
}

/// Multi-if value-correctness: sequential ifs with accumulation.
/// Game-of-Life neighbor-count shape: conditionally add from two arrays.
/// a=[1,2,3,4], b=[10,20,30,40]
/// i=0: a[0]=1>0 (add), b[0]=10 not>15 → sum=1
/// i=1: a[1]=2>0 (add), b[1]=20>15 (add) → sum=2+20=22
/// i=2: a[2]=3>0 (add), b[2]=30>15 (add) → sum=3+30=33
/// i=3: a[3]=4>0 (add), b[3]=40>15 (add) → sum=4+40=44
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn multi_if_sequential_with_accumulation_value_correctness() {
    let source = "
use system.gpu
use system.collections.array

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
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "1 22 33 44");
}

/// Multi-if value-correctness: nested ifs with accumulation.
/// Box-blur-like shape: nested bounds guard accumulation.
/// a=[1,2,3,4], b=[10,20,30,40]
/// i=0: a[0]=1>1 (no) → sum=0
/// i=1: a[1]=2>1 (yes), sum=2; b[1]=20<30 (yes), sum=2+20=22
/// i=2: a[2]=3>1 (yes), sum=3; b[2]=30<30 (no) → sum=3
/// i=3: a[3]=4>1 (yes), sum=4; b[3]=40<30 (no) → sum=4
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn multi_if_nested_with_accumulation_value_correctness() {
    let source = "
use system.gpu
use system.collections.array

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
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "0 22 3 4");
}

/// Multi-if value-correctness: if-else statement.
/// MOST CRITICAL: verify the else body actually emits and runs.
/// a=[1,2,3,4]
/// i=0: a[0]=1 not>2 → else: result=0
/// i=1: a[1]=2 not>2 → else: result=0
/// i=2: a[2]=3>2 → if: result=1
/// i=3: a[3]=4>2 → if: result=1
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn multi_if_else_value_correctness() {
    let source = "
use system.gpu
use system.collections.array

gpu let a = [1, 2, 3, 4]
gpu var dst = [0, 0, 0, 0]
gpu forall i in 0..4
    var result = 0
    if a[i] > 2
        result = 1
    else
        result = 0
    dst[i] = result
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "0 0 1 1");
}

/// 2D gpu forall value-correctness test.
/// A 2D grid `gpu forall x, y in 0..4, 0..3` over a 4x3 array (12 elements total).
/// Each thread computes `dst[y * 4 + x] = x + y`.
/// Expected output after readback: row r, column c → value c+r.
/// Row 0: 0 1 2 3
/// Row 1: 1 2 3 4
/// Row 2: 2 3 4 5
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_for_2d_value_round_trips_through_device() {
    let source = "
use system.gpu
use system.collections.array

gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
gpu forall x, y in 0..4, 0..3
    dst[y * 4 + x] = x + y
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]} {host[5]} {host[6]} {host[7]} {host[8]} {host[9]} {host[10]} {host[11]}')
";
    assert_gpu_runs_with_output(source, "0 1 2 3 1 2 3 4 2 3 4 5");
}

/// 2D gpu forall with runtime bounds compiles, dispatches, and produces correct values.
/// The loop bounds are variables (w, h) determined at runtime, not compile-time constants.
/// Verifies that over-dispatch safety works: with non-multiple-of-16 bounds (5x3),
/// the kernel only writes to valid cells (0..15 of a 20-cell buffer), leaving
/// cells 15-19 untouched (initialized to 99).
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_for_2d_runtime_bounds_value_round_trips() {
    let source = "
use system.gpu
use system.collections.array

fn main()
    let w = 5
    let h = 3
    gpu var dst = [99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99]
    gpu forall x, y in 0..w, 0..h
        dst[y * 5 + x] = x * 100 + y
    let host = dst
    println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]} {host[5]} {host[6]} {host[7]} {host[15]} {host[16]}')
";
    assert_gpu_runs_with_output(source, "0 100 200 300 400 1 101 201 99 99");
}

/// 2D gpu forall with mixed literal x-end and runtime y-end.
/// Verifies that when one axis is literal and the other is runtime, both bounds
/// are correctly materialized and passed to the kernel.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_for_2d_mixed_literal_x_runtime_y_bounds() {
    let source = "
use system.gpu
use system.collections.array

fn main()
    let h = 3
    gpu var buf = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall x, y in 0..4, 0..h
        buf[y * 4 + x] = x * 100 + y
    let r = buf
    println(f'{r[0]} {r[1]} {r[6]} {r[11]}')
";
    // buf layout: [row0, row1, row2]
    // row0 (y=0): [x*100+0 for x=0..4] = [0, 100, 200, 300]
    // row1 (y=1): [x*100+1 for x=0..4] = [1, 101, 201, 301]
    // row2 (y=2): [x*100+2 for x=0..4] = [2, 102, 202, 302]
    // r[0]=0, r[1]=100, r[6]=buf[1*4+2]=201, r[11]=buf[2*4+3]=302
    assert_gpu_runs_with_output(source, "0 100 201 302");
}

/// 2D gpu forall with mixed runtime x-end and literal y-end.
/// Verifies that when the x-axis is runtime and y-axis is literal, both bounds
/// are correctly materialized and passed to the kernel.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_for_2d_mixed_runtime_x_literal_y_bounds() {
    let source = "
use system.gpu
use system.collections.array

fn main()
    let w = 4
    gpu var buf = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall x, y in 0..w, 0..3
        buf[y * 4 + x] = x * 100 + y
    let r = buf
    println(f'{r[0]} {r[1]} {r[6]} {r[11]}')
";
    // buf layout: [row0, row1, row2]
    // row0 (y=0): [x*100+0 for x=0..w] = [0, 100, 200, 300]
    // row1 (y=1): [x*100+1 for x=0..w] = [1, 101, 201, 301]
    // row2 (y=2): [x*100+2 for x=0..w] = [2, 102, 202, 302]
    // r[0]=0, r[1]=100, r[6]=buf[1*4+2]=201, r[11]=buf[2*4+3]=302
    assert_gpu_runs_with_output(source, "0 100 201 302");
}

/// Buffer ping-pong with 3 generations and telemetry.
/// Two persistent `gpu var` grids swapped across 3 sequential `forall` kernels
/// WITHOUT intermediate readback. Proves:
/// - Buffers are reused across launches
/// - Read-only vs read-write capture (each kernel declares the binding it needs)
/// Acceptance criterion: final value is correct AND gpu_readbacks() == 1 (only final readback).
/// Sequence:
/// 1. First kernel: b[i] = a[i] + 100  (a read-only, b read-write)
/// 2. Second kernel: a[i] = b[i] + 1000 (b read-only, a read-write)
/// 3. Third kernel: b[i] = a[i] + 100  (a read-only, b read-write)
/// Final b[i] = ((a[i]+100)+1000)+100 = a[i]+1200 → b=[1201,1202,1203,1204]
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn ping_pong_three_generations_value_and_telemetry() {
    let source = "
use system.gpu
use system.collections.array

gpu_reset_telemetry()
gpu var a = [1, 2, 3, 4]
gpu var b = [0, 0, 0, 0]
gpu forall i in 0..4
    b[i] = a[i] + 100
gpu forall i in 0..4
    a[i] = b[i] + 1000
gpu forall i in 0..4
    b[i] = a[i] + 100
let host = b
println(f'{host[0]} {host[1]} {host[2]} {host[3]} {gpu_readbacks()}')
";
    assert_gpu_runs_with_output(source, "1201 1202 1203 1204 1");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_i64_modulo_roundtrips() {
    let source = "
use system.gpu
use system.collections.array

gpu let src = [0, 1, 2, 3, 4]
gpu var dst = [0, 0, 0, 0, 0]
gpu forall i in 0..5
    dst[i] = src[i] % 3

let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]}')
";
    assert_gpu_runs_with_output(source, "0 1 2 0 1");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_i64_divide_roundtrips() {
    let source = "
use system.gpu
use system.collections.array

gpu let src = [0, 1, 2, 3, 4]
gpu var dst = [0, 0, 0, 0, 0]
gpu forall i in 0..5
    dst[i] = src[i] / 2

let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]}')
";
    assert_gpu_runs_with_output(source, "0 0 1 1 2");
}

#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_i64_arithmetic_kernel_still_works() {
    let source = "
use system.gpu
use system.collections.array

gpu let src = [10, 20, 30, 40]
gpu var dst = [0, 0, 0, 0]
gpu forall i in 0..4
    dst[i] = src[i] + 5

let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "15 25 35 45");
}

/// Test both div and mod in the same kernel using flat addressing (row = i / 3, col = i % 3).
/// This verifies the Metal MSL i64 narrowing workaround fires for both operators.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_i64_div_and_mod_flat_addressing() {
    let source = "
use system.gpu
use system.collections.array

gpu let indices = [0, 1, 2, 3, 4, 5, 6, 7, 8]
gpu var rows = [0, 0, 0, 0, 0, 0, 0, 0, 0]
gpu var cols = [0, 0, 0, 0, 0, 0, 0, 0, 0]

gpu forall i in 0..9
    rows[i] = indices[i] / 3
    cols[i] = indices[i] % 3

let h_rows = rows
let h_cols = cols
println(f'{h_rows[0]} {h_rows[1]} {h_rows[2]} {h_rows[3]} {h_rows[4]} {h_rows[5]} {h_rows[6]} {h_rows[7]} {h_rows[8]}')
println(f'{h_cols[0]} {h_cols[1]} {h_cols[2]} {h_cols[3]} {h_cols[4]} {h_cols[5]} {h_cols[6]} {h_cols[7]} {h_cols[8]}')
";
    assert_gpu_runs_with_output(source, "0 0 0 1 1 1 2 2 2\n0 1 2 0 1 2 0 1 2");
}

/// Test int→float cast inside a gpu kernel.
/// The kernel casts i64 loop counter to f32 and stores into f32 buffer.
/// Uses explicit `i as f32` to match the f32 buffer width.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_cast_int_to_float_in_kernel() {
    let source = "
use system.gpu
use system.collections.array

gpu var posx = [0.0, 0.0, 0.0, 0.0]

gpu forall i in 0..4
    posx[i] = i as f32 * 0.25

let h = posx
println(f'{h[0]} {h[1]} {h[2]} {h[3]}')
";
    assert_gpu_runs_with_output(source, "0 0.25 0.5 0.75");
}

/// Test float→int cast inside a gpu kernel.
/// The kernel applies floor() to an f32 value and casts to i64.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_cast_float_to_int_in_kernel() {
    let source = "
use system.gpu
use system.collections.array
use system.math

gpu let src = [1.7, 2.3, 3.9]
gpu var idx = [0, 0, 0]

gpu forall i in 0..3
    idx[i] = floor(src[i]) as int

let h = idx
println(f'{h[0]} {h[1]} {h[2]}')
";
    assert_gpu_runs_with_output(source, "1 2 3");
}

// NOTE: the former `gpu_i64_divide_large_constant` / `gpu_i64_modulo_large_constant`
// tests were removed. They asserted the old i64-constant-narrowing behaviour, where a
// near-`i64::MAX` constant in a kernel was silently truncated into i32 range. GPU
// integers are now i32 end-to-end (WebGPU/WGSL has no 64-bit integer type), so a
// constant exceeding i32 range is genuinely unrepresentable in a kernel. In-range
// integer div/mod stays covered by `gpu_i64_divide_roundtrips` / `gpu_i64_modulo_roundtrips`.
// TODO: reject an out-of-i32-range integer constant inside a GPU kernel with a
// clean compile-time error instead of the current shader-compile abort.

/// Test negative dividend with division (i32 semantics: truncate toward zero).
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_i64_divide_negative() {
    let source = "
use system.gpu
use system.collections.array

gpu let src = [0, 0-1, 0-2, 3, 4]
gpu var dst = [0, 0, 0, 0, 0]

gpu forall i in 0..5
    dst[i] = src[i] / 2

let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]}')
";
    // Signed division truncate toward zero:
    // 0 / 2 = 0, -1 / 2 = 0 (not -1), -2 / 2 = -1, 3 / 2 = 1, 4 / 2 = 2
    assert_gpu_runs_with_output(source, "0 0 -1 1 2");
}

/// Test negative dividend with modulo (sign follows dividend).
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn gpu_i64_modulo_negative() {
    let source = "
use system.gpu
use system.collections.array

gpu let src = [0, 0-1, 0-2, 3, 4]
gpu var dst = [0, 0, 0, 0, 0]

gpu forall i in 0..5
    dst[i] = src[i] % 3

let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]} {host[4]}')
";
    // Modulo with negative dividend (sign follows dividend):
    // 0 % 3 = 0, -1 % 3 = -1, -2 % 3 = -2, 3 % 3 = 0, 4 % 3 = 1
    assert_gpu_runs_with_output(source, "0 -1 -2 0 1");
}

/// End-to-end value correctness for short-circuit `or` in a kernel body.
/// Each thread flags an element when it equals the first or last value.
/// a = [1, 2, 3, 4]: only indices 0 (==1) and 3 (==4) match → 100, rest 0.
/// A wrongly-inverted condition would flip the result, so the exact pattern
/// proves the `false`-target negation is correct, not just naga-valid.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn or_condition_value_round_trips_through_device() {
    let source = "
use system.gpu
use system.collections.array

gpu let a = [1, 2, 3, 4]
gpu var dst = [0, 0, 0, 0]
gpu forall i in 0..4
    var flag = 0
    if a[i] == 1 or a[i] == 4
        flag = 100
    dst[i] = flag
let host = dst
println(f'{host[0]} {host[1]} {host[2]} {host[3]}')
";
    assert_gpu_runs_with_output(source, "100 0 0 100");
}

// NOTE: Game of Life correctness test is not included here because multiple if-else
// statements in GPU kernels currently hit a SwitchInt limitation in the WGSL structurizer.
// The demo test (in tests/integration/gpu/demos.rs) will serve as the acceptance criterion
// for rule correctness once it is created and value-verified on Metal.
