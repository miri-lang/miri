// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Function metadata tracking for type checking.
//!
//! This module provides the [`FunctionAnalysis`] struct, which encapsulates
//! function bodies, parameter residency information, and GPU callability analysis
//! that were previously directly on [`TypeChecker`].
//!
//! [`TypeChecker`]: super::TypeChecker

use super::FnResidency;
use crate::ast::Statement;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

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
    /// function declared with the `runtime` keyword. A program function may
    /// share a runtime function's name, so the declaration a reference reached
    /// — never the name — is what says whether its callee is the C export.
    pub(crate) runtime_references: HashSet<usize>,
    /// Every identifier expression, by id, that resolved in its scope to a
    /// function declared with the `intrinsic` keyword — including one a module
    /// body reaches that the program's own imports leave out of its scope.
    pub(crate) intrinsic_references: HashSet<usize>,
}

impl FunctionAnalysis {
    /// Creates a new function analysis tracker.
    pub(crate) fn new() -> Self {
        Self {
            function_bodies: HashMap::new(),
            function_out_params: HashMap::new(),
            fn_residencies: HashMap::new(),
            runtime_references: HashSet::new(),
            intrinsic_references: HashSet::new(),
        }
    }
}
