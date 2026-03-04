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
                        | TypeKind::Array(_, _)
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

        // Process each block with a single-pass rebuild to avoid O(n^2) Vec::insert.
        for block_data in &mut body.basic_blocks {
            let old_stmts = std::mem::take(&mut block_data.statements);
            let mut new_stmts = Vec::with_capacity(old_stmts.len());
            let mut block_changed = false;

            for stmt in old_stmts {
                match &stmt.kind {
                    StatementKind::Assign(_lhs, rvalue) => {
                        if let Some(place) = get_rvalue_source_place(rvalue) {
                            if managed_locals.contains(&place.local) {
                                new_stmts.push(Statement {
                                    kind: StatementKind::IncRef(place),
                                    span: stmt.span,
                                });
                                block_changed = true;
                            }
                        }
                    }
                    StatementKind::StorageDead(place) => {
                        if managed_locals.contains(&place.local) {
                            new_stmts.push(Statement {
                                kind: StatementKind::DecRef(place.clone()),
                                span: stmt.span,
                            });
                            block_changed = true;
                        }
                    }
                    StatementKind::StorageLive(_)
                    | StatementKind::Nop
                    | StatementKind::IncRef(_)
                    | StatementKind::DecRef(_)
                    | StatementKind::Dealloc(_) => {}
                }
                new_stmts.push(stmt);
            }

            if block_changed {
                changed = true;
            }
            block_data.statements = new_stmts;
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
