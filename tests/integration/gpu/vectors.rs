// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for generic GPU vector types Vec2, Vec3, Vec4.
//! This file focuses on inline value-type construction and field access
//! on the CPU before GPU kernels are involved.

use super::utils::*;

/// Vec3<f32> construction and field access on CPU.
#[test]
fn cpu_vec3_f32_construction_and_access() {
    let source = "
use system.gpu.vector

fn main()
    let v = Vec3<f32>(1.0, 2.0, 3.0)
    println(f'{v.x} {v.y} {v.z}')
";
    assert_runs_with_output(source, "1.0 2.0 3.0");
}

/// Vec2<i32> construction and field access on CPU.
#[test]
fn cpu_vec2_i32_construction_and_access() {
    let source = "
use system.gpu.vector

fn main()
    let v = Vec2<i32>(42, 99)
    println(f'{v.x} {v.y}')
";
    assert_runs_with_output(source, "42 99");
}

/// Vec4<u32> construction and field access on CPU.
#[test]
fn cpu_vec4_u32_construction_and_access() {
    let source = "
use system.gpu.vector

fn main()
    let v = Vec4<u32>(10, 20, 30, 40)
    println(f'{v.x} {v.y} {v.z} {v.w}')
";
    assert_runs_with_output(source, "10 20 30 40");
}

/// Vec3<f32> field assignment with mutable binding.
#[test]
fn cpu_vec3_mutable_field_assignment() {
    let source = "
use system.gpu.vector

fn main()
    var v = Vec3<f32>(1.0, 2.0, 3.0)
    v.x = 5.0
    v.y = 6.0
    println(f'{v.x} {v.y} {v.z}')
";
    assert_runs_with_output(source, "5.0 6.0 3.0");
}

/// Array of Vec3<f32> stored inline: construct, read fields across elements,
/// drop without leak. Distinct per-element values prove elements don't overlap.
#[test]
fn cpu_vec3_array_construct_and_field_read() {
    let source = "
use system.gpu.vector
use system.collections.array

fn main()
    let arr = [Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0), Vec3<f32>(7.0, 8.0, 9.0)]
    println(f'{arr[0].x} {arr[1].y} {arr[2].z}')
";
    assert_runs_with_output(source, "1.0 5.0 9.0");
}

/// Array of Vec4<f32> stored inline: every component of every element reads
/// back correctly (stride 16 == payload 16, no padding).
#[test]
fn cpu_vec4_array_construct_and_field_read() {
    let source = "
use system.gpu.vector
use system.collections.array

fn main()
    let arr = [Vec4<f32>(1.0, 2.0, 3.0, 4.0), Vec4<f32>(5.0, 6.0, 7.0, 8.0)]
    println(f'{arr[0].x} {arr[0].w} {arr[1].x} {arr[1].w}')
";
    assert_runs_with_output(source, "1.0 4.0 5.0 8.0");
}

/// Inline vector element field write: `arr[i].x = e` updates one component and
/// leaves the others intact (no 8-byte pointer store corrupting neighbors).
#[test]
fn cpu_vec3_array_field_write() {
    let source = "
use system.gpu.vector
use system.collections.array

fn main()
    var arr = [Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)]
    arr[0].x = 9.0
    arr[1].z = 8.0
    println(f'{arr[0].x} {arr[0].y} {arr[1].z}')
";
    assert_runs_with_output(source, "9.0 2.0 8.0");
}

/// Inline whole-element write: `arr[i] = VecN(...)` replaces all components.
#[test]
fn cpu_vec3_array_whole_element_write() {
    let source = "
use system.gpu.vector
use system.collections.array

fn main()
    var arr = [Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)]
    arr[0] = Vec3<f32>(7.0, 8.0, 9.0)
    println(f'{arr[0].x} {arr[0].y} {arr[0].z} {arr[1].x}')
";
    assert_runs_with_output(source, "7.0 8.0 9.0 4.0");
}

/// Inline element-to-element copy `dst[i] = src[i]` (the CPU analogue of the
/// GPU forall copy) moves the full std430 element.
#[test]
fn cpu_vec3_array_element_copy() {
    let source = "
use system.gpu.vector
use system.collections.array

fn main()
    let src = [Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)]
    var dst = [Vec3<f32>(0.0, 0.0, 0.0), Vec3<f32>(0.0, 0.0, 0.0)]
    dst[0] = src[0]
    dst[1] = src[1]
    println(f'{dst[0].x} {dst[0].y} {dst[0].z} {dst[1].x} {dst[1].y} {dst[1].z}')
";
    assert_runs_with_output(source, "1.0 2.0 3.0 4.0 5.0 6.0");
}

/// Integer vector arrays preserve their explicit component width: `[Vec2<i32>(..)]`
/// keeps `i32` (not the literal default `Int`), so inline storage is detected and
/// each component reads back correctly across distinct elements.
#[test]
fn cpu_integer_vec_array_preserves_width() {
    let source = "
use system.gpu.vector
use system.collections.array

fn main()
    let a = [Vec2<i32>(10, 20), Vec2<i32>(30, 40), Vec2<i32>(50, 60)]
    let b = [Vec4<u32>(1, 2, 3, 4), Vec4<u32>(5, 6, 7, 8)]
    println(f'{a[0].x} {a[1].y} {a[2].x} {b[0].x} {b[1].w}')
";
    assert_runs_with_output(source, "10 40 50 1 8");
}

/// Vec2 component-wise arithmetic.
#[test]
fn cpu_vec2_arithmetic() {
    let source = "
use system.gpu.vector

fn main()
    let v1 = Vec2<f32>(1.0, 2.0)
    let v2 = Vec2<f32>(3.0, 4.0)
    let x_sum = v1.x + v2.x
    let y_sum = v1.y + v2.y
    println(f'{x_sum} {y_sum}')
";
    assert_runs_with_output(source, "4.0 6.0");
}

/// Vec3<f32> type-checks as GPU-compatible in a forall kernel.
#[test]
fn vec3_f32_is_gpu_compatible() {
    let source = "
use system.gpu.vector
use system.collections.array

fn main()
    gpu var v = Vec3<f32>(1.0, 2.0, 3.0)
    let x = v.x
";
    assert_runs(source);
}

/// Vec2<i32> type-checks as GPU-compatible in a forall kernel.
#[test]
fn vec2_i32_is_gpu_compatible() {
    let source = "
use system.gpu.vector
use system.collections.array

fn main()
    gpu var v = Vec2<i32>(1, 2)
    let x = v.x
";
    assert_runs(source);
}

/// Vec4<u32> type-checks as GPU-compatible in a forall kernel.
#[test]
fn vec4_u32_is_gpu_compatible() {
    let source = "
use system.gpu.vector
use system.collections.array

fn main()
    gpu var v = Vec4<u32>(1, 2, 3, 4)
    let x = v.x
";
    assert_runs(source);
}

/// Vec3<f64> is NOT GPU-compatible (f64 has no WGSL vector support).
#[test]
fn vec3_f64_is_not_gpu_compatible() {
    let source = "
use system.gpu.vector

gpu fn my_kernel(v Vec3<f64>)
    let x = v.x
";
    assert_compiler_error(source, "not GPU-compatible");
}

/// Vec3<i64> is NOT GPU-compatible (i64 has no WGSL vector support).
#[test]
fn vec3_i64_is_not_gpu_compatible() {
    let source = "
use system.gpu.vector

gpu fn my_kernel(v Vec3<i64>)
    let x = v.x
";
    assert_compiler_error(source, "not GPU-compatible");
}

/// Vec3<u64> is NOT GPU-compatible (u64 has no WGSL vector support).
#[test]
fn vec3_u64_is_not_gpu_compatible() {
    let source = "
use system.gpu.vector

gpu fn my_kernel(v Vec3<u64>)
    let x = v.x
";
    assert_compiler_error(source, "not GPU-compatible");
}

/// Vec3<f32> elements in a buffer emit valid WGSL.
#[test]
fn vec3_f32_array_emits_valid_wgsl() {
    use super::helpers::assert_gpu_wgsl_valid;
    let source = "
use system.gpu.vector
use system.collections.array

gpu fn my_kernel(src Array<Vec3<f32>, 2>)
    let v = src[0]
    let x = v.x

fn main()
    let arr = [Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)]
    my_kernel(arr)
";
    assert_gpu_wgsl_valid(source);
}

/// Array<Vec3<f32>, N> round-trip: copy via GPU forall (value correctness check).
/// Vector elements are stored inline in the buffer with std430 stride; adapter-gated.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec3_f32_array_buffer_roundtrip() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let src = [Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)]
    gpu var dst = [Vec3<f32>(0.0, 0.0, 0.0), Vec3<f32>(0.0, 0.0, 0.0)]
    gpu forall i in 0..2
        dst[i] = src[i]
    let host = dst
    println(f'{host[0].x} {host[0].y} {host[0].z} {host[1].x} {host[1].y} {host[1].z}')
";
    assert_gpu_runs_with_output(source, "1.0 2.0 3.0 4.0 5.0 6.0");
}

/// std430 alignment proof: 3 distinct `Vec3<f32>` elements round-trip via GPU
/// forall; element[2] must read from byte offset 32 (stride 16), not 24 — if
/// the stride were packed (12), element[2]'s components would bleed/shift.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec3_f32_array_std430_stride_alignment() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let src = [Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0), Vec3<f32>(7.0, 8.0, 9.0)]
    gpu var dst = [Vec3<f32>(0.0, 0.0, 0.0), Vec3<f32>(0.0, 0.0, 0.0), Vec3<f32>(0.0, 0.0, 0.0)]
    gpu forall i in 0..3
        dst[i] = src[i]
    let host = dst
    println(f'{host[2].x} {host[2].y} {host[2].z}')
";
    assert_gpu_runs_with_output(source, "7.0 8.0 9.0");
}

/// Vec2<i32> buffer round-trip (stride 8 == ptr size); adapter-gated.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec2_i32_array_buffer_roundtrip() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let src = [Vec2<i32>(10, 20), Vec2<i32>(30, 40)]
    gpu var dst = [Vec2<i32>(0, 0), Vec2<i32>(0, 0)]
    gpu forall i in 0..2
        dst[i] = src[i]
    let host = dst
    println(f'{host[0].x} {host[0].y} {host[1].x} {host[1].y}')
";
    assert_gpu_runs_with_output(source, "10 20 30 40");
}

/// Vec4<u32> buffer round-trip (stride 16); adapter-gated.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec4_u32_array_buffer_roundtrip() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let src = [Vec4<u32>(1, 2, 3, 4), Vec4<u32>(5, 6, 7, 8)]
    gpu var dst = [Vec4<u32>(0, 0, 0, 0), Vec4<u32>(0, 0, 0, 0)]
    gpu forall i in 0..2
        dst[i] = src[i]
    let host = dst
    println(f'{host[0].x} {host[0].y} {host[0].z} {host[0].w} {host[1].x} {host[1].y} {host[1].z} {host[1].w}')
";
    assert_gpu_runs_with_output(source, "1 2 3 4 5 6 7 8");
}

/// dot(Vec3<f32>, Vec3<f32>) -> f32 WGSL validity (type checking).
#[test]
fn vec_dot_f32_wgsl_valid() {
    use super::helpers::assert_gpu_wgsl_valid;
    let source = "
use system.gpu
use system.gpu.vector
use system.math
use system.collections.array

fn main()
    gpu var dummy = [0.0]
    gpu forall i in 0..1
        let a = Vec3<f32>(1.0, 0.0, 0.0)
        let b = Vec3<f32>(2.0, 3.0, 4.0)
        dummy[i] = dot(a, b)
";
    assert_gpu_wgsl_valid(source);
}

/// Value correctness: dot([1,0,0], [2,3,4]) = 2.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec_dot_f32_value_correct() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.math
use system.collections.array

fn main()
    gpu let ax = [1.0]
    gpu let ay = [0.0]
    gpu let az = [0.0]
    gpu let bx = [2.0]
    gpu let by = [3.0]
    gpu let bz = [4.0]
    gpu var result = [0.0]
    gpu forall i in 0..1
        let a = Vec3<f32>(ax[i], ay[i], az[i])
        let b = Vec3<f32>(bx[i], by[i], bz[i])
        result[i] = dot(a, b)
    let host = result
    println(f'{host[0]}')
";
    assert_gpu_runs_with_output(source, "2.0");
}

/// length(Vec3<f32>) -> f32 WGSL validity and value correctness.
#[test]

fn vec_length_f32_wgsl_valid() {
    use super::helpers::assert_gpu_wgsl_valid;
    let source = "
use system.gpu
use system.gpu.vector
use system.math
use system.collections.array

fn main()
    gpu let vx = [3.0]
    gpu let vy = [4.0]
    gpu let vz = [0.0]
    gpu var result = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        result[i] = length(v)
";
    assert_gpu_wgsl_valid(source);
}

/// Value correctness: length([3,4,0]) = 5.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec_length_f32_value_correct() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.math
use system.collections.array

fn main()
    gpu let vx = [3.0]
    gpu let vy = [4.0]
    gpu let vz = [0.0]
    gpu var result = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        result[i] = length(v)
    let host = result
    println(f'{host[0]}')
";
    assert_gpu_runs_with_output(source, "5.0");
}

/// normalize(Vec3<f32>) -> Vec3<f32> WGSL validity and value correctness.
#[test]

fn vec_normalize_f32_wgsl_valid() {
    use super::helpers::assert_gpu_wgsl_valid;
    let source = "
use system.gpu
use system.gpu.vector
use system.math
use system.collections.array

fn main()
    gpu let vx = [3.0]
    gpu let vy = [4.0]
    gpu let vz = [0.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        let n = normalize(v)
        rx[i] = n.x
        ry[i] = n.y
        rz[i] = n.z
";
    assert_gpu_wgsl_valid(source);
}

/// Value correctness: normalize([3,4,0]) = [0.6, 0.8, 0].
#[test]
#[ignore = "f32 GPU results widen to f64 on host readback: array literals like [0.0] infer as Array<f64> because the float literal parser cannot yet encode a size constraint; f32 kernel results are stored into f64 arrays and print at f64 precision. Fix requires per-buffer f32 residency typing or syntax like [0.0f32] to narrow at declaration time."]
fn vec_normalize_f32_value_correct() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.math
use system.collections.array

fn main()
    gpu let vx = [3.0]
    gpu let vy = [4.0]
    gpu let vz = [0.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        let n = normalize(v)
        rx[i] = n.x
        ry[i] = n.y
        rz[i] = n.z
    let host_x = rx
    let host_y = ry
    let host_z = rz
    println(f'{host_x[0]} {host_y[0]} {host_z[0]}')
";
    assert_gpu_runs_with_output(source, "0.6 0.8 0.0");
}

/// cross(Vec3<f32>, Vec3<f32>) -> Vec3<f32> WGSL validity and value correctness.
#[test]

fn vec_cross_f32_wgsl_valid() {
    use super::helpers::assert_gpu_wgsl_valid;
    let source = "
use system.gpu
use system.gpu.vector
use system.math
use system.collections.array

fn main()
    gpu let ax = [1.0]
    gpu let ay = [0.0]
    gpu let az = [0.0]
    gpu let bx = [0.0]
    gpu let by = [1.0]
    gpu let bz = [0.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let a = Vec3<f32>(ax[i], ay[i], az[i])
        let b = Vec3<f32>(bx[i], by[i], bz[i])
        let c = cross(a, b)
        rx[i] = c.x
        ry[i] = c.y
        rz[i] = c.z
";
    assert_gpu_wgsl_valid(source);
}

/// Value correctness: cross([1,0,0], [0,1,0]) = [0,0,1].
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec_cross_f32_value_correct() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.math
use system.collections.array

fn main()
    gpu let ax = [1.0]
    gpu let ay = [0.0]
    gpu let az = [0.0]
    gpu let bx = [0.0]
    gpu let by = [1.0]
    gpu let bz = [0.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let a = Vec3<f32>(ax[i], ay[i], az[i])
        let b = Vec3<f32>(bx[i], by[i], bz[i])
        let c = cross(a, b)
        rx[i] = c.x
        ry[i] = c.y
        rz[i] = c.z
    let host_x = rx
    let host_y = ry
    let host_z = rz
    println(f'{host_x[0]} {host_y[0]} {host_z[0]}')
";
    assert_gpu_runs_with_output(source, "0.0 0.0 1.0");
}

/// reflect(Vec3<f32>, Vec3<f32>) -> Vec3<f32> WGSL validity and value correctness.
#[test]

fn vec_reflect_f32_wgsl_valid() {
    use super::helpers::assert_gpu_wgsl_valid;
    let source = "
use system.gpu
use system.gpu.vector
use system.math
use system.collections.array

fn main()
    gpu let ix = [1.0]
    gpu let iy = [0.0]
    gpu let iz = [0.0]
    gpu let nx = [0.0]
    gpu let ny = [1.0]
    gpu let nz = [0.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(ix[i], iy[i], iz[i])
        let n = Vec3<f32>(nx[i], ny[i], nz[i])
        let r = reflect(v, n)
        rx[i] = r.x
        ry[i] = r.y
        rz[i] = r.z
";
    assert_gpu_wgsl_valid(source);
}

/// Value correctness: reflect([1,0,0], [0,1,0]) = [1,0,0].
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec_reflect_f32_value_correct() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.math
use system.collections.array

fn main()
    gpu let ix = [1.0]
    gpu let iy = [0.0]
    gpu let iz = [0.0]
    gpu let nx = [0.0]
    gpu let ny = [1.0]
    gpu let nz = [0.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(ix[i], iy[i], iz[i])
        let n = Vec3<f32>(nx[i], ny[i], nz[i])
        let r = reflect(v, n)
        rx[i] = r.x
        ry[i] = r.y
        rz[i] = r.z
    let host_x = rx
    let host_y = ry
    let host_z = rz
    println(f'{host_x[0]} {host_y[0]} {host_z[0]}')
";
    assert_gpu_runs_with_output(source, "1.0 0.0 0.0");
}

/// mix(Vec3<f32>, Vec3<f32>, f32) -> Vec3<f32> WGSL validity and value correctness.
#[test]

fn vec_mix_f32_wgsl_valid() {
    use super::helpers::assert_gpu_wgsl_valid;
    let source = "
use system.gpu
use system.gpu.vector
use system.math
use system.collections.array

fn main()
    gpu let ax = [0.0]
    gpu let ay = [0.0]
    gpu let az = [0.0]
    gpu let bx = [2.0]
    gpu let by = [2.0]
    gpu let bz = [2.0]
    gpu let t = [0.5]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let a = Vec3<f32>(ax[i], ay[i], az[i])
        let b = Vec3<f32>(bx[i], by[i], bz[i])
        let m = mix(a, b, t[i])
        rx[i] = m.x
        ry[i] = m.y
        rz[i] = m.z
";
    assert_gpu_wgsl_valid(source);
}

/// Value correctness: mix([0,0,0], [2,2,2], 0.5) = [1,1,1].
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec_mix_f32_value_correct() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.math
use system.collections.array

fn main()
    gpu let ax = [0.0]
    gpu let ay = [0.0]
    gpu let az = [0.0]
    gpu let bx = [2.0]
    gpu let by = [2.0]
    gpu let bz = [2.0]
    gpu let t = [0.5]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let a = Vec3<f32>(ax[i], ay[i], az[i])
        let b = Vec3<f32>(bx[i], by[i], bz[i])
        let m = mix(a, b, t[i])
        rx[i] = m.x
        ry[i] = m.y
        rz[i] = m.z
    let host_x = rx
    let host_y = ry
    let host_z = rz
    println(f'{host_x[0]} {host_y[0]} {host_z[0]}')
";
    assert_gpu_runs_with_output(source, "1.0 1.0 1.0");
}

/// dot(Vec3<i32>, Vec3<i32>) should be rejected with clear error.
#[test]
fn vec_dot_i32_rejected() {
    let source = "
use system.gpu.vector
use system.math

gpu fn my_kernel(a Vec3<i32>, b Vec3<i32>)
    let result = dot(a, b)
";
    assert_compiler_error(source, "dot");
}

/// reflect(Vec3<i32>, Vec3<i32>) should be rejected — reflect requires f32 elements.
#[test]
fn vec_reflect_i32_rejected() {
    let source = "
use system.gpu.vector
use system.math

gpu fn my_kernel(a Vec3<i32>, b Vec3<i32>)
    let result = reflect(a, b)
";
    assert_compiler_error(source, "reflect");
}

/// mix(Vec3<i32>, Vec3<i32>, i32) should be rejected — mix requires f32 elements.
#[test]
fn vec_mix_i32_rejected() {
    let source = "
use system.gpu.vector
use system.math

gpu fn my_kernel(a Vec3<i32>, b Vec3<i32>, t i32)
    let result = mix(a, b, t)
";
    assert_compiler_error(source, "mix");
}

/// cross(Vec2<f32>, Vec2<f32>) should be rejected with clear error.
#[test]
fn vec_cross_vec2_rejected() {
    let source = "
use system.gpu.vector
use system.math

gpu fn my_kernel(a Vec2<f32>, b Vec2<f32>)
    let result = cross(a, b)
";
    assert_compiler_error(source, "cross");
}

/// cross(Vec4<f32>, Vec4<f32>) should be rejected with clear error.
#[test]
fn vec_cross_vec4_rejected() {
    let source = "
use system.gpu.vector
use system.math

gpu fn my_kernel(a Vec4<f32>, b Vec4<f32>)
    let result = cross(a, b)
";
    assert_compiler_error(source, "cross");
}

/// dot(Vec2<f32>, Vec3<f32>) dimension mismatch should be rejected.
#[test]
fn vec_dot_dim_mismatch_rejected() {
    let source = "
use system.gpu.vector
use system.math

gpu fn my_kernel(a Vec2<f32>, b Vec3<f32>)
    let result = dot(a, b)
";
    assert_compiler_error(source, "dot");
}

/// reflect(Vec2<f32>, Vec3<f32>) dimension mismatch should be rejected.
#[test]
fn vec_reflect_dim_mismatch_rejected() {
    let source = "
use system.gpu.vector
use system.math

gpu fn my_kernel(a Vec2<f32>, b Vec3<f32>)
    let result = reflect(a, b)
";
    assert_compiler_error(source, "reflect");
}

/// mix(Vec2<f32>, Vec3<f32>, f32) dimension mismatch should be rejected.
#[test]
fn vec_mix_dim_mismatch_rejected() {
    let source = "
use system.gpu.vector
use system.math

gpu fn my_kernel(a Vec2<f32>, b Vec3<f32>, t f32)
    let result = mix(a, b, t)
";
    assert_compiler_error(source, "mix");
}

/// Vec3<f32> * f32 scalar broadcast WGSL validity and value correctness.
#[test]

fn vec_scalar_mul_f32_wgsl_valid() {
    use super::helpers::assert_gpu_wgsl_valid;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let vx = [1.0]
    gpu let vy = [2.0]
    gpu let vz = [3.0]
    gpu let scalar = [2.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        let result = v * scalar[i]
        rx[i] = result.x
        ry[i] = result.y
        rz[i] = result.z
";
    assert_gpu_wgsl_valid(source);
}

/// Value correctness: [1,2,3] * 2.0 = [2,4,6].
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec_scalar_mul_f32_value_correct() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let vx = [1.0]
    gpu let vy = [2.0]
    gpu let vz = [3.0]
    gpu let scalar = [2.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        let result = v * scalar[i]
        rx[i] = result.x
        ry[i] = result.y
        rz[i] = result.z
    let host_x = rx
    let host_y = ry
    let host_z = rz
    println(f'{host_x[0]} {host_y[0]} {host_z[0]}')
";
    assert_gpu_runs_with_output(source, "2.0 4.0 6.0");
}

/// Vec3<f32> / f32 scalar broadcast WGSL validity and value correctness.
#[test]

fn vec_scalar_div_f32_wgsl_valid() {
    use super::helpers::assert_gpu_wgsl_valid;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let vx = [2.0]
    gpu let vy = [4.0]
    gpu let vz = [6.0]
    gpu let scalar = [2.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        let result = v / scalar[i]
        rx[i] = result.x
        ry[i] = result.y
        rz[i] = result.z
";
    assert_gpu_wgsl_valid(source);
}

/// Value correctness: [2,4,6] / 2.0 = [1,2,3].
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec_scalar_div_f32_value_correct() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let vx = [2.0]
    gpu let vy = [4.0]
    gpu let vz = [6.0]
    gpu let scalar = [2.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        let result = v / scalar[i]
        rx[i] = result.x
        ry[i] = result.y
        rz[i] = result.z
    let host_x = rx
    let host_y = ry
    let host_z = rz
    println(f'{host_x[0]} {host_y[0]} {host_z[0]}')
";
    assert_gpu_runs_with_output(source, "1.0 2.0 3.0");
}

/// Vec3<f32> + f32 scalar broadcast WGSL validity and value correctness.
#[test]

fn vec_scalar_add_f32_wgsl_valid() {
    use super::helpers::assert_gpu_wgsl_valid;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let vx = [1.0]
    gpu let vy = [2.0]
    gpu let vz = [3.0]
    gpu let scalar = [1.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        let result = v + scalar[i]
        rx[i] = result.x
        ry[i] = result.y
        rz[i] = result.z
";
    assert_gpu_wgsl_valid(source);
}

/// Value correctness: [1,2,3] + 1.0 = [2,3,4].
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec_scalar_add_f32_value_correct() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let vx = [1.0]
    gpu let vy = [2.0]
    gpu let vz = [3.0]
    gpu let scalar = [1.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        let result = v + scalar[i]
        rx[i] = result.x
        ry[i] = result.y
        rz[i] = result.z
    let host_x = rx
    let host_y = ry
    let host_z = rz
    println(f'{host_x[0]} {host_y[0]} {host_z[0]}')
";
    assert_gpu_runs_with_output(source, "2.0 3.0 4.0");
}

/// Vec3<f32> - f32 scalar broadcast WGSL validity and value correctness.
#[test]

fn vec_scalar_sub_f32_wgsl_valid() {
    use super::helpers::assert_gpu_wgsl_valid;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let vx = [2.0]
    gpu let vy = [3.0]
    gpu let vz = [4.0]
    gpu let scalar = [1.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        let result = v - scalar[i]
        rx[i] = result.x
        ry[i] = result.y
        rz[i] = result.z
";
    assert_gpu_wgsl_valid(source);
}

/// Value correctness: [2,3,4] - 1.0 = [1,2,3].
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec_scalar_sub_f32_value_correct() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let vx = [2.0]
    gpu let vy = [3.0]
    gpu let vz = [4.0]
    gpu let scalar = [1.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        let result = v - scalar[i]
        rx[i] = result.x
        ry[i] = result.y
        rz[i] = result.z
    let host_x = rx
    let host_y = ry
    let host_z = rz
    println(f'{host_x[0]} {host_y[0]} {host_z[0]}')
";
    assert_gpu_runs_with_output(source, "1.0 2.0 3.0");
}

/// f32 * Vec3<f32> scalar broadcast (commutative) WGSL validity and value correctness.
#[test]

fn vec_scalar_mul_commutative_f32_wgsl_valid() {
    use super::helpers::assert_gpu_wgsl_valid;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let vx = [1.0]
    gpu let vy = [2.0]
    gpu let vz = [3.0]
    gpu let scalar = [2.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        let result = scalar[i] * v
        rx[i] = result.x
        ry[i] = result.y
        rz[i] = result.z
";
    assert_gpu_wgsl_valid(source);
}

/// Value correctness: 2.0 * [1,2,3] = [2,4,6].
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec_scalar_mul_commutative_f32_value_correct() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let vx = [1.0]
    gpu let vy = [2.0]
    gpu let vz = [3.0]
    gpu let scalar = [2.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        let result = scalar[i] * v
        rx[i] = result.x
        ry[i] = result.y
        rz[i] = result.z
    let host_x = rx
    let host_y = ry
    let host_z = rz
    println(f'{host_x[0]} {host_y[0]} {host_z[0]}')
";
    assert_gpu_runs_with_output(source, "2.0 4.0 6.0");
}

/// Integer vector local in a GPU kernel should emit valid WGSL with i32 zero literals.
#[test]
fn vec_integer_zero_init_emits_valid_wgsl() {
    use super::helpers::assert_gpu_wgsl_valid;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu var buf = [0]
    gpu forall i in 0..1
        var v = Vec3<i32>(1, 2, 3)
        buf[i] = v.x
";
    assert_gpu_wgsl_valid(source);
}

/// f32 + Vec3<f32> scalar broadcast (commutative) WGSL validity and value correctness.
#[test]

fn vec_scalar_add_commutative_f32_wgsl_valid() {
    use super::helpers::assert_gpu_wgsl_valid;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let vx = [1.0]
    gpu let vy = [2.0]
    gpu let vz = [3.0]
    gpu let scalar = [1.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        let result = scalar[i] + v
        rx[i] = result.x
        ry[i] = result.y
        rz[i] = result.z
";
    assert_gpu_wgsl_valid(source);
}

/// Value correctness: 1.0 + [1,2,3] = [2,3,4].
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn vec_scalar_add_commutative_f32_value_correct() {
    use super::device::assert_gpu_runs_with_output;
    let source = "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let vx = [1.0]
    gpu let vy = [2.0]
    gpu let vz = [3.0]
    gpu let scalar = [1.0]
    gpu var rx = [0.0]
    gpu var ry = [0.0]
    gpu var rz = [0.0]
    gpu forall i in 0..1
        let v = Vec3<f32>(vx[i], vy[i], vz[i])
        let result = scalar[i] + v
        rx[i] = result.x
        ry[i] = result.y
        rz[i] = result.z
    let host_x = rx
    let host_y = ry
    let host_z = rz
    println(f'{host_x[0]} {host_y[0]} {host_z[0]}')
";
    assert_gpu_runs_with_output(source, "2.0 3.0 4.0");
}
