<!-- SPDX-License-Identifier: Apache-2.0 -->
<!-- Copyright (c) Viacheslav Shynkarenko -->

# Miri GPU Demo Programs

This directory contains production-grade example programs showcasing Miri's GPU residency surface (Milestone 6.5). Each demo is a single `.mi` source file that demonstrates a specific GPU programming pattern within the constraints of the current compiler surface.

## Design notes

- **Single source of truth**: These files are loaded by the test suite via `include_str!("../examples/gpu/<name>.mi")`, ensuring demo code and CI tests always verify identical bytes.
- **Surface constraints**: Demos use only features shipped in M6.5 Phase 1: `gpu let`/`gpu var`/`gpu for` with literal Int range bounds, no kernel-body loops, straight-line code + `if`-guards only, and cross-residency readback via assignment.
- **§17 compliance**: Each demo's source comments justify itself against the six-point GPU code review checklist in `notes/GPU_DRAFT.md` §17 (residency, cost-classes, buffer-reuse, mutability, bounds+indexing, portability).

## Demos

### vector_add.mi

**Purpose**: Demonstrates the basic GPU residency surface: two immutable gpu-resident float arrays captured by a kernel, element-wise sum into a mutable device buffer, and cross-residency readback.

**Cost class sequence** (per §17.2):
1. Upload: two captured `gpu let` arrays + one `gpu var`, marshalled to device on first kernel launch
2. Launch: one `gpu for i in 0..4` kernel
3. Readback: one cross-residency assignment `let host = dst`, which fences and copies the result back

**Expected output**: `6.0 8.0 10.0 12.0` (1.0+5.0, 2.0+6.0, 3.0+7.0, 4.0+8.0).

**§17 compliance**:
- §17.1 residency: `gpu let a`, `gpu let b`, `gpu var dst` are all GPU-resident.
- §17.2 cost-classes: upload → launch → readback, in order.
- §17.3 buffer-reuse: single kernel; no reuse.
- §17.4 mutability: `dst[i]` written by thread i only.
- §17.5 bounds: loop `0..4` covers all elements; no guards.
- §17.6 portability: pure float arithmetic; no backend-specific code.

### saxpy.mi

**Purpose**: Fused multiply-add with a literal scalar constant inlined in the kernel. Demonstrates inline scalar math in a GPU kernel without uniform or push-constant machinery.

**Cost class sequence**:
1. Upload: two captured `gpu let` arrays + one `gpu var`
2. Launch: one `gpu for i in 0..4` kernel computing `dst[i] = a[i] * 2.0 + b[i]`
3. Readback: one cross-residency assignment

**Expected output**: `7.0 10.0 13.0 16.0` (1.0*2.0+5.0, 2.0*2.0+6.0, 3.0*2.0+7.0, 4.0*2.0+8.0).

**§17 compliance**:
- §17.1 residency: `gpu let a`, `gpu let b`, `gpu var dst`.
- §17.2 cost-classes: upload → launch → readback.
- §17.3 buffer-reuse: single kernel.
- §17.4 mutability: `dst[i]` written by thread i only.
- §17.5 bounds: loop covers all; literal scalar is always safe.
- §17.6 portability: pure arithmetic; no backend code.

### buffer_reuse.mi

**Purpose**: Demonstrates persistent device buffer reuse: two sequential `gpu for` blocks on the same `gpu var` with no readback between them. Proves the cost model: one upload at first capture, two launches, one readback at the end.

**Cost class sequence**:
1. Upload: one `gpu var data`, allocated and uploaded to device at first kernel capture
2. Launch (kernel 1): first `gpu for i in 0..8` block initializes `data[i] = i`
3. Launch (kernel 2): second `gpu for i in 0..8` block modifies the same buffer: `data[i] = data[i] + 8`
4. Readback: one cross-residency assignment `let host = data` at the end

The device buffer is NOT deallocated between kernels (§16.2 persistent model); the second kernel reads and writes the same buffer.

**Expected output**: `15 1 2 1 1` (host[7] = 7+8 = 15, then telemetry showing 1 upload, 2 launches, 1 readback, 1 fence).

**§17 compliance**:
- §17.1 residency: `gpu var data` only.
- §17.2 cost-classes: upload (first kernel) → launch → launch → readback, in order. No readback between kernels.
- §17.3 buffer-reuse: two adjacent kernels; same `data` buffer; no cross-residency assignment between them.
- §17.4 mutability: `data[i]` written by thread i only, in each kernel.
- §17.5 bounds: loop covers all; no guards.
- §17.6 portability: pure arithmetic.

## Deferred / Dropped Demos

### map_normalize (dropped — compiler blocker)

**Blocker**: Math intrinsics (sqrt, sin, cos, etc.) in GPU kernels DO emit correct WGSL and pass naga validation, but the intrinsic's result temp is typed `f64` while storage buffers are `f32` — a scalar-width mismatch. On adapters without f64 support (e.g. Metal) the kernel produces 0 instead of 2.0. This is a type-mismatch bug in the WGSL codegen, not missing codegen (blocker: F23).

**Intended purpose** (for future restoration): Apply a math intrinsic (sqrt) element-wise to a float array to demonstrate portable math on the GPU.

**Future work** (Phase F23): Fix math-intrinsic result temps to match buffer element width (f32 buffers → f32 result temps). Once this ships, re-add `examples/gpu/map_normalize.mi` and a test.

### box_blur (not attempted)

**Reason**: 1D flat-buffer neighborhood filtering with per-neighbor bounds `if`-guards inside the kernel. Exceeds the "straight-line code + single `if`-guard" surface constraint for Phase 1. Requires either (a) lifting the structurizer limit to handle multi-`if` accumulation, or (b) adding local shared memory + barrier support.

**Intended purpose**: Demonstrate bounds-checking patterns and local accumulation in GPU kernels.

**Future work** (Phase F24): Revisit once kernel-body loop support lands and we can express neighborhood iteration cleanly.

---

**Last updated**: 2026-06-02. Demos ship with Milestone 6.5 Phase 1 (native GPU dispatch).
