// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU integration tests.
//!
//! Three test surfaces share this directory:
//!   * `stub` — host-only `GpuArray<T, Size>` wrapper. No GPU touched.
//!   * `launch` — full Cranelift → `miri_gpu_launch_inline` → `wgpu` native
//!     dispatch path. Owns end-to-end value correctness for `gpu for`
//!     kernels now that the WGSL scalar mapping aligns host and device
//!     widths (`int` → `i64`).
//!   * `wgsl` — WGSL backend driven directly from Rust via the `helpers`
//!     module: `assert_gpu_wgsl_valid` (naga validation, no hardware) and
//!     `assert_gpu_compute_i64` (wgpu dispatch with i64 buffers, skipped
//!     when no adapter exposes `Features::SHADER_INT64`). M6.5 task
//!     "Helper-shrink" is the final cleanup that drops the wgpu dispatch
//!     helper entirely and leaves only naga validation here.

pub use crate::integration::utils;

pub mod accelerable;
pub mod cross_residency;
pub mod helpers;
pub mod launch;
pub mod persistent_buffer;
pub mod residency;
pub mod stub;
pub mod wgsl;
