// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! GPU integration tests.
//!
//! Test surfaces sharing this directory:
//!   * `device` — GPU availability detection via int round-trip probe.
//!     Exposes `gpu_adapter_available()` and `assert_gpu_runs_with_output()`.
//!   * `launch` — full Cranelift → `miri_gpu_launch_inline` → `wgpu` native
//!     dispatch path. Owns end-to-end value correctness for `forall`
//!     kernels now that the WGSL scalar mapping aligns host and device
//!     widths (`int` → `i64`).
//!   * `wgsl` — WGSL backend shader validity tests via naga validation only.
//!     Uses `assert_gpu_wgsl_valid` from `helpers` (no hardware required).

pub use crate::integration::utils;

pub mod accelerable;
pub mod box_blur;
pub mod browser_validation;
pub mod bundle_validation;
pub mod cross_residency;
pub mod demos;
pub mod device;
pub mod forall_routing;
pub mod game_of_life;
pub mod gpu_for_capture_residency;
pub mod gpu_frame;
pub mod helpers;
pub mod i32_range_validation;
pub mod kernel_callable_fns;
pub mod launch;
pub mod launch_3d;
pub mod math_kernels;
pub mod persistent_buffer;
pub mod ping_pong;
pub mod read_only_capture;
pub mod residency;
pub mod scalar_capture;
pub mod sized_constructor;
pub mod variable_bound_gpu_for;
pub mod wgsl;
