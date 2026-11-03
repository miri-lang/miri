// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The program-wide MIR passes GPU code needs once every body is lowered:
//! the GPU copies of the functions kernels call, and the workgroup size each
//! kernel runs with.

pub mod device_helpers;
pub mod workgroups;

pub use device_helpers::clone_gpu_device_helpers;
pub use workgroups::{stamp_kernel_workgroups, WorkgroupError};
