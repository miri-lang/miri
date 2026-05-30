// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Miri GPU runtime.
//!
//! Independent Cargo crate providing the wgpu host driver for Miri's
//! `gpu fn` / `gpu for` constructs. Linked only when a target enables
//! GPU codegen — the core Miri binary stays free of heavy GPU deps.
//!
//! The compiler talks to this crate through the FFI declared in
//! `src/stdlib/system/gpu.mi` with the `runtime "gpu" fn` keyword.

pub mod buffer;
pub mod compute;
pub mod context;
pub mod device_table;
pub mod launch;
pub mod telemetry;

pub use buffer::*;
pub use compute::*;
pub use context::*;
pub use device_table::*;
pub use launch::*;
pub use telemetry::*;
