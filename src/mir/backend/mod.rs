// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Backend-specific types for MIR.
//!
//! This module contains types that are specific to particular backends (GPU, TPU, etc.).
//! These are isolated here to keep the core MIR types backend-agnostic.

pub mod gpu;

pub use gpu::*;
