// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

pub mod args;
pub mod version;

pub use args::{BuildTarget, Cli, ColorMode, Commands, CpuBackend, Format};
pub use version::{version_ref, version_string};
