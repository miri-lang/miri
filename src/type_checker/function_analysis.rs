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
use super::module_loader::PROGRAM_MODULE;
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

/// Which source file a declaration is made in: the program's own file, or a
/// module a `use` loaded. The program's file is told apart by how it was
/// loaded, never by a name, so a module a `use` names `Main` — or anything
/// else — is never taken for it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ModuleId {
    /// The program's own file, which no `use` names.
    Program,
    /// A module a `use` loaded, by the identifiers of the path it named the
    /// module by: `["local", "geometry", "shapes"]`, `["system", "math"]`.
    Imported(Vec<String>),
}

impl ModuleId {
    /// The module a `use` loaded as `path`: `system.math`.
    pub(crate) fn imported(path: &str) -> Self {
        ModuleId::Imported(path.split('.').map(str::to_string).collect())
    }

    /// The module declarations made while checking under the module name
    /// `module` belong to. The program's file is checked under a name no
    /// `use` path can spell, and every loaded module under the path its
    /// `use` wrote.
    pub(crate) fn checked_as(module: &str) -> Self {
        if module == PROGRAM_MODULE {
            ModuleId::Program
        } else {
            Self::imported(module)
        }
    }

    /// The identifiers of the path a `use` names this module by; none for
    /// the program's own file.
    pub fn path(&self) -> &[String] {
        match self {
            ModuleId::Program => &[],
            ModuleId::Imported(path) => path,
        }
    }
}

/// A top-level function as its declaration identifies it: the module that
/// declares it and the name it is declared under there. Two modules may each
/// declare a function of one name; they are two functions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeclaredFunction {
    /// The module the function is declared in.
    pub module: ModuleId,
    /// The name the function is declared under, which an import alias does
    /// not change.
    pub name: String,
}

impl DeclaredFunction {
    /// The function a name written as `written` resolved to, `info` being
    /// the declaration it resolved to.
    pub(crate) fn resolved(written: &str, info: &SymbolInfo) -> Self {
        Self {
            module: ModuleId::checked_as(&info.module),
            name: info.original_name.as_deref().unwrap_or(written).to_string(),
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
    /// Every identifier expression, by id, that resolved in its scope to a
    /// [`CalleeKind::Program`] function, with the declaration it resolved to.
    pub(crate) declared_callees: HashMap<usize, DeclaredFunction>,
    /// The module of every top-level function declaration, by statement id,
    /// that a module the program imports declares.
    pub(crate) declaring_modules: HashMap<usize, ModuleId>,
}

impl FunctionAnalysis {
    /// Creates a new function analysis tracker.
    pub(crate) fn new() -> Self {
        Self {
            function_bodies: HashMap::new(),
            function_out_params: HashMap::new(),
            fn_residencies: HashMap::new(),
            callee_kinds: HashMap::new(),
            declared_callees: HashMap::new(),
            declaring_modules: HashMap::new(),
        }
    }
}
