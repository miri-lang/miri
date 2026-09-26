// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Function metadata tracking for type checking.
//!
//! This module provides the [`FunctionAnalysis`] struct, which encapsulates
//! function bodies, parameter residency information, and GPU callability analysis
//! that were previously directly on [`TypeChecker`].
//!
//! [`TypeChecker`]: super::TypeChecker

use super::context::SymbolInfo;
use super::FnResidency;
use crate::ast::Statement;
use std::collections::HashMap;
use std::rc::Rc;

/// How a call to a name is compiled, as the declaration the name resolved to
/// where it was written says — never as the name is spelled, since a program
/// may declare a function under any name a runtime export or an intrinsic has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalleeKind {
    /// A function the runtime library exports, declared with `runtime`.
    Runtime,
    /// A declaration made `intrinsic`, whose meaning the compiler supplies;
    /// its name says which intrinsic it is.
    Intrinsic,
    /// A function this compilation lowers, or a name that is no function.
    Program,
}

impl CalleeKind {
    /// The kind of callee the declaration `info` declares.
    pub(crate) fn of(info: &SymbolInfo) -> Self {
        if info.is_runtime {
            CalleeKind::Runtime
        } else if info.is_intrinsic {
            CalleeKind::Intrinsic
        } else {
            CalleeKind::Program
        }
    }
}

/// Function metadata tracking for GPU analysis and call site validation.
///
/// This struct encapsulates all function-metadata fields that were previously
/// directly on [`TypeChecker`], providing better separation of concerns.
///
/// [`TypeChecker`]: super::TypeChecker
#[derive(Debug)]
pub(crate) struct FunctionAnalysis {
    /// Maps user-defined function names to their Statement bodies for GPU callability analysis.
    pub(crate) function_bodies: HashMap<String, Rc<Statement>>,
    /// Maps function names to a Vec<bool> of their parameters' `is_out` flags.
    /// Populated during function declaration checking; used in GPU kernel launch
    /// to determine which buffers are writable.
    pub(crate) function_out_params: HashMap<String, Vec<bool>>,
    /// Computed residency verdict for each function (HostOnly or PolymorphicSafe).
    /// Populated during function declaration checking; used at call sites to
    /// determine if gpu-resident args are allowed.
    pub(crate) fn_residencies: HashMap<String, FnResidency>,
    /// Every identifier expression, by id, that resolved in its scope to a
    /// declaration made `runtime` or `intrinsic` — including one a module body
    /// reaches that the program's own imports leave out of its scope. Every
    /// other expression names a [`CalleeKind::Program`] callee.
    pub(crate) callee_kinds: HashMap<usize, CalleeKind>,
}

impl FunctionAnalysis {
    /// Creates a new function analysis tracker.
    pub(crate) fn new() -> Self {
        Self {
            function_bodies: HashMap::new(),
            function_out_params: HashMap::new(),
            fn_residencies: HashMap::new(),
            callee_kinds: HashMap::new(),
        }
    }
}
