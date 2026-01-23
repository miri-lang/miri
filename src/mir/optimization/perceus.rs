// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::types::TypeKind;
use crate::mir::optimization::OptimizationPass;
use crate::mir::statement::{Statement, StatementKind};
use crate::mir::{Body, Place, Rvalue};

/// Perceus: Precise Reference Counting and Reuse.
///
/// This pass inserts IncRef and DecRef operations for managed types.
/// It implements the "Functional But In-Place" strategy where possible.
pub struct Perceus;

impl OptimizationPass for Perceus {
    fn run(&mut self, body: &mut Body) -> bool {
        let mut changed = false;
        let mut new_blocks = Vec::new();

        for block_data in &body.basic_blocks {
            let mut new_statements = Vec::new();

            for stmt in &block_data.statements {
                match &stmt.kind {
                    StatementKind::Assign(_lhs, rvalue) => {
                        // Check if we need to insert IncRef
                        if let Some(place) = self.get_rvalue_source_place(rvalue) {
                            if self.is_managed(body, &place) {
                                // Insert IncRef(rhs) before assignment
                                // In a full implementation, we check liveness to skip this if it's a move.
                                new_statements.push(Statement {
                                    kind: StatementKind::IncRef(place.clone()),
                                    span: stmt.span.clone(),
                                });
                                changed = true;
                            }
                        }

                        // Add the assignment itself
                        new_statements.push(stmt.clone());
                    }
                    StatementKind::StorageDead(place) => {
                        // Before variable goes out of scope, DecRef it
                        if self.is_managed(body, place) {
                            new_statements.push(Statement {
                                kind: StatementKind::DecRef(place.clone()),
                                span: stmt.span.clone(),
                            });
                            changed = true;
                        }
                        new_statements.push(stmt.clone());
                    }
                    _ => {
                        new_statements.push(stmt.clone());
                    }
                }
            }

            let mut new_block = block_data.clone();
            new_block.statements = new_statements;
            new_blocks.push(new_block);
        }

        if changed {
            body.basic_blocks = new_blocks;
        }

        changed
    }

    fn name(&self) -> &'static str {
        "Perceus"
    }
}

impl Perceus {
    fn is_managed(&self, body: &Body, place: &Place) -> bool {
        // Look up type of place in local_decls
        // Look up type of place in local_decls
        if let Some(local_decl) = body.local_decls.get(place.local.0) {
            // Managed if not primitive and not linear
            // (Linear types are manually managed/moved, not RC)
            // (Primitives are Copy)
            // Strings, Lists, Maps are Managed.
            matches!(
                &local_decl.ty.kind,
                TypeKind::String
                    | TypeKind::List(_)
                    | TypeKind::Map(_, _)
                    | TypeKind::Set(_)
                    | TypeKind::Custom(_, _)
            )
        } else {
            false
        }
    }

    fn get_rvalue_source_place(&self, rvalue: &Rvalue) -> Option<Place> {
        match rvalue {
            Rvalue::Use(op) => match op {
                crate::mir::Operand::Copy(p) | crate::mir::Operand::Move(p) => Some(p.clone()),
                crate::mir::Operand::Constant(_) => None,
            },
            Rvalue::Ref(place) => Some(place.clone()),
            _ => None,
        }
    }
}
