// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The names the bodies GPU code reaches are declared under in a WGSL module.
//!
//! WGSL admits identifier characters only, so a kernel's entry point and every
//! helper a kernel calls are declared under [`Symbol::wgsl_name`] rather than
//! their link name. That spelling joins a definition's parts with `_`, which a
//! program's identifiers may contain too, so it is not injective: `A.b_c` and
//! `A_b.c` are both `A_b_c`. Every helper is emitted into the module of every
//! kernel, so each of these bodies claims its WGSL name in one table, and two
//! distinct definitions spelling one name refuse the program at compile time
//! rather than failing to validate when the kernel is launched.
//!
//! The data emitted beside a kernel for the host to launch it by are spelled
//! from its entry-point name with two fixed, distinct suffixes, so they are
//! distinct whenever the entry points are.

use std::collections::HashSet;

use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::symbol::{Namespace, Symbol, SymbolTable};
use crate::mir::{Body, ExecutionModel};

/// The WGSL names claimed so far, and the bodies GPU code reaches as
/// helpers.
#[derive(Debug)]
pub struct GpuNames {
    claimed: SymbolTable,
    helpers: HashSet<Symbol>,
}

impl GpuNames {
    /// Claim the WGSL name of every kernel among `lowered`.
    pub fn collect(lowered: &[(Symbol, Body)]) -> Result<Self, LoweringError> {
        let mut names = Self {
            claimed: SymbolTable::new(Namespace::Wgsl),
            helpers: HashSet::new(),
        };
        for (symbol, body) in lowered {
            match body.execution_model {
                ExecutionModel::GpuKernel => {
                    names.claimed.claim_at(symbol, body.span)?;
                }
                ExecutionModel::Cpu | ExecutionModel::GpuDevice | ExecutionModel::Async => {}
            }
        }
        Ok(names)
    }

    /// Claim the WGSL name of the body `symbol`, defined at `span`, which GPU
    /// code reaches as a helper: the name its GPU clone is declared under.
    pub fn claim_helper(&mut self, symbol: &Symbol, span: Span) -> Result<(), LoweringError> {
        self.claimed.claim_at(symbol, span)?;
        self.helpers.insert(symbol.clone());
        Ok(())
    }

    /// The bodies claimed as helpers, which a WGSL module declares under
    /// their WGSL names and its calls name by them.
    pub fn helpers(&self) -> &HashSet<Symbol> {
        &self.helpers
    }

    /// The claimed helpers, handed to the backends that spell calls to them.
    pub fn into_helpers(self) -> HashSet<Symbol> {
        self.helpers
    }

    /// The name a lowered body is emitted under; see [`spelled_name`].
    pub fn emitted_name(&self, symbol: &Symbol, body: &Body) -> String {
        spelled_name(&self.helpers, symbol, body.execution_model)
    }
}

/// The name the body `symbol` runs as under `model` is declared and called
/// by, `helpers` being the bodies claimed as device helpers: a GPU kernel's is
/// the entry point its WGSL module declares, which the host launches it by,
/// and a helper's GPU clone is declared under its WGSL name; every other
/// body's is its link name.
///
/// A call from GPU code names its callee as the device body it runs as, so a
/// declaration and every call to it spell one name.
pub fn spelled_name(helpers: &HashSet<Symbol>, symbol: &Symbol, model: ExecutionModel) -> String {
    match model {
        ExecutionModel::GpuKernel => symbol.wgsl_name(),
        ExecutionModel::GpuDevice if helpers.contains(symbol) => symbol.wgsl_name(),
        ExecutionModel::Cpu | ExecutionModel::GpuDevice | ExecutionModel::Async => {
            symbol.link_name()
        }
    }
}
