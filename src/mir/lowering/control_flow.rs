// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Control flow lowering — thin re-export facade.
//!
//! The actual implementations live in focused sub-modules:
//! - `loops`        — if, while, for, break, continue
//! - `dispatch`     — method dispatch, name mangling, `lower_call`
//! - `constructors` — struct and class constructors

pub use super::constructors::{lower_class_constructor, lower_struct_constructor};
pub use super::dispatch::lower_call;
pub(crate) use super::dispatch::mangle_generic_name;
pub use super::loops::{lower_break, lower_continue, lower_for, lower_if, lower_while};
