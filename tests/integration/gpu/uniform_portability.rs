// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Portability of the runtime range-bound / range-start control uniforms beyond
// Metal. A `forall` whose range end (or start) is a runtime `Int` lowers the
// value into the kernel through a bare-scalar `var<uniform> _bound_x: u32;`
// (and `_start_x` for a runtime start). Those declarations are known to parse,
// validate, and run on Metal; these tests prove they also lower cleanly for the
// Vulkan and D3D backends.
//
// naga is wgpu's shader translator on every native backend, so lowering the
// kernel through naga's SPIR-V writer (the Vulkan shader form) and HLSL writer
// (the D3D12 shader form) exercises the exact code path a real Vulkan/D3D launch
// takes. If a bare-scalar uniform needed a struct wrap or 16-byte pad on those
// backends, one of these lowerings would fail. They succeed, so no padding is
// required.

use super::helpers::{compile_kernel_to_hlsl, compile_kernel_to_spirv};

/// 1D runtime range end → a single `_bound_x` u32 uniform.
const RUNTIME_BOUND_1D: &str = "
use system.gpu
use system.collections.array

fn main()
    let n = 4
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..n
        dst[i] = a[i]
";

/// 2D runtime range ends → two uniforms, `_bound_x` and `_bound_y`.
const RUNTIME_BOUND_2D: &str = "
use system.gpu
use system.collections.array

fn main()
    let w = 5
    let h = 3
    gpu var dst = [99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99, 99]
    gpu forall x, y in 0..w, 0..h
        dst[y * 5 + x] = x * 100 + y
";

/// Runtime range start → adds a `_start_x` u32 uniform alongside `_bound_x`.
const RUNTIME_START_1D: &str = "
use system.gpu
use system.collections.array

fn main()
    let a = 1
    let n = 4
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in a..n
        dst[i] = i
";

/// The `_bound_x` uniform lowers to valid Vulkan SPIR-V.
#[test]
fn runtime_bound_1d_lowers_to_vulkan_spirv() {
    let spirv = compile_kernel_to_spirv(RUNTIME_BOUND_1D);
    assert!(
        !spirv.is_empty(),
        "expected a non-empty SPIR-V word stream for the runtime-bound kernel"
    );
}

/// The `_bound_x` uniform lowers to valid D3D12 HLSL. naga wraps a bare-scalar
/// uniform in a `cbuffer`, so the declaration must appear in the output.
#[test]
fn runtime_bound_1d_lowers_to_d3d_hlsl() {
    let hlsl = compile_kernel_to_hlsl(RUNTIME_BOUND_1D);
    assert!(
        hlsl.contains("cbuffer _bound_x"),
        "expected the bound uniform to lower into a D3D constant buffer, got:\n{}",
        hlsl
    );
}

/// Two bound uniforms (`_bound_x`, `_bound_y`) lower cleanly to both backends.
#[test]
fn runtime_bound_2d_two_uniforms_lower_to_both() {
    assert!(!compile_kernel_to_spirv(RUNTIME_BOUND_2D).is_empty());
    let hlsl = compile_kernel_to_hlsl(RUNTIME_BOUND_2D);
    assert!(
        hlsl.contains("cbuffer _bound_x"),
        "missing x bound cbuffer:\n{}",
        hlsl
    );
    assert!(
        hlsl.contains("cbuffer _bound_y"),
        "missing y bound cbuffer:\n{}",
        hlsl
    );
}

/// The runtime range-start uniform (`_start_x`) is the same bare-scalar shape and
/// must also lower to both backends.
#[test]
fn runtime_start_uniform_lowers_to_both() {
    assert!(!compile_kernel_to_spirv(RUNTIME_START_1D).is_empty());
    let hlsl = compile_kernel_to_hlsl(RUNTIME_START_1D);
    assert!(
        hlsl.contains("cbuffer _start_x"),
        "missing start cbuffer:\n{}",
        hlsl
    );
    assert!(
        hlsl.contains("cbuffer _bound_x"),
        "missing bound cbuffer:\n{}",
        hlsl
    );
}
