<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright (c) Viacheslav Shynkarenko -->

# Miri GPU Demo Programs

This directory contains production-grade example programs showcasing Miri's GPU residency surface. Each demo is a single `.mi` source file that demonstrates a specific GPU programming pattern within the constraints of the current compiler surface.

## Design notes

- **Single source of truth**: These files are loaded by the test suite via `include_str!("../../../examples/gpu/<name>.mi")`, ensuring demo code and CI tests always verify identical bytes.
- **Surface constraints**: Demos use only currently-available GPU features: `gpu let`/`gpu var`/`gpu for` with literal Int range bounds, no kernel-body loops, straight-line code + `if`-guards only, and cross-residency readback via assignment.
- **Correctness checklist**: Each demo validates residency tracking, upload/launch/readback sequencing, buffer lifetime, write safety (no data races), bounds correctness, and portability (no backend-specific code).

## Demos

### vector_add.mi

**Purpose**: Demonstrates the basic GPU residency surface: two immutable gpu-resident float arrays captured by a kernel, element-wise sum into a mutable device buffer, and cross-residency readback.

**Execution model**:
1. Upload: two captured `gpu let` arrays + one `gpu var`, marshalled to device on first kernel launch
2. Launch: one `gpu for i in 0..4` kernel
3. Readback: one cross-residency assignment `let host = dst`, which fences and copies the result back

**Expected output**: `6.0 8.0 10.0 12.0` (1.0+5.0, 2.0+6.0, 3.0+7.0, 4.0+8.0).

**Correctness properties**:
- **Residency**: `gpu let a`, `gpu let b`, `gpu var dst` are all GPU-resident.
- **Cost sequencing**: upload → launch → readback, in order (no intermediate readbacks).
- **Buffer reuse**: single kernel; no persistent buffer reuse.
- **Data safety**: `dst[i]` written by thread i only (no write conflicts).
- **Bounds safety**: loop `0..4` covers all elements; no out-of-bounds guards needed.
- **Portability**: pure float arithmetic; no backend-specific code.

### saxpy.mi

**Purpose**: Fused multiply-add with a literal scalar constant inlined in the kernel. Demonstrates inline scalar math in a GPU kernel without uniform or push-constant machinery.

**Execution model**:
1. Upload: two captured `gpu let` arrays + one `gpu var`
2. Launch: one `gpu for i in 0..4` kernel computing `dst[i] = a[i] * 2.0 + b[i]`
3. Readback: one cross-residency assignment

**Expected output**: `7.0 10.0 13.0 16.0` (1.0*2.0+5.0, 2.0*2.0+6.0, 3.0*2.0+7.0, 4.0*2.0+8.0).

**Correctness properties**:
- **Residency**: `gpu let a`, `gpu let b`, `gpu var dst` all GPU-resident.
- **Cost sequencing**: upload → launch → readback (no intermediate readbacks).
- **Buffer reuse**: single kernel; no buffer reuse across kernels.
- **Data safety**: `dst[i]` written by thread i only (no write conflicts).
- **Bounds safety**: loop covers all elements; literal scalar is always safe.
- **Portability**: pure arithmetic; no backend-specific code.

### buffer_reuse.mi

**Purpose**: Demonstrates persistent device buffer reuse: two sequential `gpu for` blocks on the same `gpu var` with no readback between them. Proves the cost model: one upload at first capture, two launches, one readback at the end.

**Execution model**:
1. Upload: one `gpu var data`, allocated and uploaded to device at first kernel capture
2. Launch (kernel 1): first `gpu for i in 0..8` block initializes `data[i] = i`
3. Launch (kernel 2): second `gpu for i in 0..8` block modifies the same buffer: `data[i] = data[i] + 8`
4. Readback: one cross-residency assignment `let host = data` at the end

The device buffer persists between kernels (no deallocation); the second kernel reads and writes the same persistent buffer.

**Expected output**: `15 1 2 1 1` (host[7] = 7+8 = 15, then telemetry showing 1 upload, 2 launches, 1 readback, 1 fence).

**Correctness properties**:
- **Residency**: `gpu var data` only (one mutable device buffer).
- **Cost sequencing**: upload (first kernel) → launch → launch → readback, in order. No readback between kernels.
- **Buffer reuse**: two adjacent kernels share the same device buffer; no intermediate cross-residency assignments.
- **Data safety**: `data[i]` written by thread i only, within each kernel.
- **Bounds safety**: loop covers all elements; no out-of-bounds guards needed.
- **Portability**: pure arithmetic; no backend-specific code.

## Future Demo Programs

### map_normalize

**Status**: Not yet included — pending fix to math intrinsics in GPU kernels.

**Issue**: Math intrinsics (sqrt, sin, cos, etc.) in GPU kernels emit correct WGSL and pass naga validation, but the intrinsic's result temp is typed `f64` while storage buffers are `f32` — a scalar-width mismatch. On adapters without f64 support (e.g. Metal) the kernel produces 0 instead of the correct result.

**Intended purpose**: Apply a math intrinsic (sqrt) element-wise to a float array to demonstrate portable math on the GPU.

**Prerequisite**: Fix the math-intrinsic result-type narrowing so result temporaries match buffer element width (f32 buffers → f32 result temps).

### box_blur

**Status**: Not yet attempted — exceeds current surface constraints.

**Constraint**: 1D flat-buffer neighborhood filtering with per-neighbor bounds `if`-guards inside the kernel exceeds the current "straight-line code + single `if`-guard" surface. Correct implementation requires either (a) lifting the WGSL structurizer limit to handle multi-`if` accumulation, or (b) adding local shared memory + barrier support.

**Intended purpose**: Demonstrate bounds-checking patterns and local accumulation in GPU kernels.

**Prerequisite**: Kernel-body loop support and multi-`if` structurizer improvements.

---

**Last updated**: 2026-06-02. Current demos showcase the native GPU dispatch surface (gpu let/var/for).
