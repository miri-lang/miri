// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Perceus: Precise Reference Counting and Reuse.
//!
//! This pass inserts `IncRef` and `DecRef` operations for managed (heap-allocated)
//! types such as `String`, `List`, `Map`, `Set`, and user-defined types.
//! It implements the "Functional But In-Place" (FBIP) strategy where possible.

use crate::ast::types::TypeKind;
use crate::mir::optimization::OptimizationPass;
use crate::mir::statement::{Statement, StatementKind};
use crate::mir::{Body, Operand, Place, Rvalue};

/// Inserts reference counting operations for managed types.
///
/// For each assignment whose source is a managed place, an `IncRef` is inserted
/// before the assignment. For each `StorageDead` of a managed place, a `DecRef`
/// is inserted before the storage is released.
pub struct Perceus;

impl OptimizationPass for Perceus {
    fn run(&mut self, body: &mut Body) -> bool {
        let mut changed = false;

        // Pre-compute which locals are managed to avoid borrow conflicts
        // between iterating basic_blocks mutably and reading local_decls.
        let managed_locals: std::collections::HashSet<crate::mir::Local> = body
            .local_decls
            .iter()
            .enumerate()
            .filter(|(_, decl)| {
                matches!(
                    &decl.ty.kind,
                    TypeKind::String
                        | TypeKind::List(_)
                        | TypeKind::Map(_, _)
                        | TypeKind::Set(_)
                        | TypeKind::Custom(_, _)
                )
            })
            .map(|(i, _)| crate::mir::Local(i))
            .collect();

        if managed_locals.is_empty() {
            return false;
        }

        // Process each block in-place by collecting insertions, then applying them.
        for block_data in &mut body.basic_blocks {
            let mut insertions: Vec<(usize, Statement)> = Vec::new();

            for (idx, stmt) in block_data.statements.iter().enumerate() {
                match &stmt.kind {
                    StatementKind::Assign(_lhs, rvalue) => {
                        if let Some(place) = get_rvalue_source_place(rvalue) {
                            if managed_locals.contains(&place.local) {
                                insertions.push((
                                    idx,
                                    Statement {
                                        kind: StatementKind::IncRef(place),
                                        span: stmt.span,
                                    },
                                ));
                            }
                        }
                    }
                    StatementKind::StorageDead(place) => {
                        if managed_locals.contains(&place.local) {
                            insertions.push((
                                idx,
                                Statement {
                                    kind: StatementKind::DecRef(place.clone()),
                                    span: stmt.span,
                                },
                            ));
                        }
                    }
                    StatementKind::StorageLive(_)
                    | StatementKind::Nop
                    | StatementKind::IncRef(_)
                    | StatementKind::DecRef(_)
                    | StatementKind::Dealloc(_) => {}
                }
            }

            if !insertions.is_empty() {
                changed = true;
                // Insert in reverse order to preserve indices
                for (idx, stmt) in insertions.into_iter().rev() {
                    block_data.statements.insert(idx, stmt);
                }
            }
        }

        changed
    }

    fn name(&self) -> &'static str {
        "Perceus"
    }
}

/// Extract the source place from an rvalue, if it references a local.
fn get_rvalue_source_place(rvalue: &Rvalue) -> Option<Place> {
    match rvalue {
        Rvalue::Use(Operand::Copy(place) | Operand::Move(place)) => Some(place.clone()),
        Rvalue::Ref(place) => Some(place.clone()),
        _ => None,
    }
}
