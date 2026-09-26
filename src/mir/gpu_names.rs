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

use std::collections::HashMap;

use crate::ast::literal::Literal;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::symbol::{Namespace, Symbol, SymbolTable};
use crate::mir::{Body, ExecutionModel, Operand, TerminatorKind};

/// The WGSL names claimed so far, and the symbols of the bodies that may yet
/// be reached from GPU code as helpers.
#[derive(Debug)]
pub struct GpuNames {
    by_link_name: HashMap<String, Symbol>,
    claimed: SymbolTable,
    helper_names: HashMap<String, String>,
}

impl GpuNames {
    /// Claim the WGSL name of every kernel among `lowered`, and remember the
    /// symbol of every other body by its link name.
    pub fn collect(lowered: &[(Symbol, Body)]) -> Result<Self, LoweringError> {
        let mut names = Self {
            by_link_name: HashMap::new(),
            claimed: SymbolTable::new(Namespace::Wgsl),
            helper_names: HashMap::new(),
        };
        for (symbol, body) in lowered {
            match body.execution_model {
                ExecutionModel::GpuKernel => {
                    names.claimed.claim_at(symbol, body.span)?;
                }
                ExecutionModel::Cpu | ExecutionModel::GpuDevice | ExecutionModel::Async => {
                    names
                        .by_link_name
                        .insert(symbol.link_name(), symbol.clone());
                }
            }
        }
        Ok(names)
    }

    /// Claim the WGSL name of the body linked as `link_name`, defined at
    /// `span`, which GPU code reaches as a helper: the name its GPU clone is
    /// declared under.
    pub fn claim_helper(&mut self, link_name: &str, span: Span) -> Result<String, LoweringError> {
        let Some(symbol) = self.by_link_name.get(link_name) else {
            return Ok(link_name.to_string());
        };
        self.claimed.claim_at(symbol, span)?;
        let wgsl_name = symbol.wgsl_name();
        self.helper_names
            .insert(link_name.to_string(), wgsl_name.clone());
        Ok(wgsl_name)
    }

    /// Point every direct call in a GPU body at the WGSL name of the helper
    /// it calls.
    pub fn retarget_calls(&self, bodies: &mut [(String, Body)]) {
        let calls = bodies
            .iter_mut()
            .filter(|(_, body)| {
                matches!(
                    body.execution_model,
                    ExecutionModel::GpuKernel | ExecutionModel::GpuDevice
                )
            })
            .flat_map(|(_, body)| body.basic_blocks.iter_mut())
            .filter_map(|block| block.terminator.as_mut());
        for terminator in calls {
            let TerminatorKind::Call {
                func: Operand::Constant(constant),
                ..
            } = &mut terminator.kind
            else {
                continue;
            };
            let Literal::Identifier(name) = &mut constant.literal else {
                continue;
            };
            if let Some(wgsl_name) = self.helper_names.get(name.as_str()) {
                name.clone_from(wgsl_name);
            }
        }
    }
}

/// The name a lowered body is emitted under: a GPU kernel's is the entry
/// point its WGSL module declares, which the host launches it by, and every
/// other body's is its link name.
pub fn emitted_name(symbol: &Symbol, body: &Body) -> String {
    match body.execution_model {
        ExecutionModel::GpuKernel => symbol.wgsl_name(),
        ExecutionModel::Cpu | ExecutionModel::GpuDevice | ExecutionModel::Async => {
            symbol.link_name()
        }
    }
}
