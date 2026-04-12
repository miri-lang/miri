// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Perceus: Precise Reference Counting and Reuse.
//!
//! This pass inserts `IncRef` and `DecRef` operations for managed (heap-allocated)
//! types such as `String`, `List`, `Map`, `Set`, and user-defined types.
//! It implements the "Functional But In-Place" (FBIP) strategy where possible.

use crate::error::syntax::Span;
use crate::mir::block::BasicBlockData;
use crate::mir::optimization::OptimizationPass;
use crate::mir::statement::{Statement, StatementKind};
use crate::mir::types::MirType;
use crate::mir::{Body, Operand, Place, PlaceElem, Rvalue};

/// Inserts reference counting operations for managed types.
///
/// For each assignment whose source is a managed place, an `IncRef` is inserted
/// before the assignment. For each `StorageDead` of a managed place, a `DecRef`
/// is inserted before the storage is released.
pub struct Perceus;
/// Metadata context for the Perceus optimization pass.
/// Holds immutable references to necessary fields of `Body` to satisfy the borrow checker
/// while iterating over `basic_blocks` mutably.
struct PerceusContext<'a> {
    local_decls: &'a [crate::mir::LocalDecl],
    auto_copy_types: &'a std::collections::HashSet<String>,
    field_types: &'a std::collections::HashMap<String, Vec<crate::ast::types::Type>>,
    type_params: &'a std::collections::HashSet<String>,
    arg_count: usize,
}

impl OptimizationPass for Perceus {
    fn run(&mut self, body: &mut Body) -> bool {
        // Step 1: Identify which variables actually need tracking (managed locals).
        let managed_locals = self.identify_managed_locals(body);
        if managed_locals.is_empty() {
            return false;
        }

        // Step 2: Iterate through every block of code and inject RC instructions.
        let mut changed = false;

        // Split the borrow: we need mutable access to basic_blocks, but only
        // immutable access to the rest of the metadata.
        let Body {
            ref mut basic_blocks,
            ref local_decls,
            ref auto_copy_types,
            ref field_types,
            ref type_params,
            arg_count,
            ..
        } = *body;

        let ctx = PerceusContext {
            local_decls,
            auto_copy_types,
            field_types,
            type_params,
            arg_count,
        };

        for block_data in basic_blocks.iter_mut() {
            if self.process_block(&ctx, block_data, &managed_locals) {
                changed = true;
            }
        }

        changed
    }

    fn name(&self) -> &'static str {
        "Perceus"
    }
}

impl Perceus {
    /// Identifies all locals that are managed (heap-allocated) and owned by this function.
    ///
    /// Excludes function parameters and "Auto-copy" types (which are small enough
    /// to be copied byte-for-byte without reference counting).
    fn identify_managed_locals(&self, body: &Body) -> std::collections::HashSet<crate::mir::Local> {
        body.local_decls
            .iter()
            .enumerate()
            .filter(|(i, decl)| {
                // Indices 1..=arg_count are function parameters; they are owned by the caller.
                *i > body.arg_count
                    && decl
                        .mir_ty
                        .is_managed(&body.auto_copy_types, &body.type_params)
            })
            .map(|(i, _)| crate::mir::Local(i))
            .collect()
    }

    /// Processes a single basic block, rebuilding its statement list with RC ops.
    ///
    /// Returns true if any new statements were inserted.
    fn process_block(
        &self,
        ctx: &PerceusContext,
        block: &mut BasicBlockData,
        managed_locals: &std::collections::HashSet<crate::mir::Local>,
    ) -> bool {
        let old_stmts = std::mem::take(&mut block.statements);
        let mut new_stmts = Vec::with_capacity(old_stmts.len());
        let mut changed = false;

        for stmt in old_stmts {
            if self.process_statement(ctx, &stmt, managed_locals, &mut new_stmts) {
                changed = true;
            }
            new_stmts.push(stmt);
        }

        block.statements = new_stmts;
        changed
    }

    /// Dispatches a statement to the appropriate RC handler.
    fn process_statement(
        &self,
        ctx: &PerceusContext,
        stmt: &Statement,
        managed_locals: &std::collections::HashSet<crate::mir::Local>,
        new_stmts: &mut Vec<Statement>,
    ) -> bool {
        match &stmt.kind {
            StatementKind::Assign(..) | StatementKind::Reassign(..) => {
                self.handle_assignment(ctx, stmt, managed_locals, new_stmts)
            }
            StatementKind::StorageDead(place) => {
                self.handle_storage_dead(stmt, place, managed_locals, new_stmts)
            }
            _ => false,
        }
    }

    /// Handles an assignment by adding IncRef for sources and DecRef for overwritten destinations.
    fn handle_assignment(
        &self,
        ctx: &PerceusContext,
        stmt: &Statement,
        managed_locals: &std::collections::HashSet<crate::mir::Local>,
        new_stmts: &mut Vec<Statement>,
    ) -> bool {
        let (lhs, rvalue, is_reassign) = match &stmt.kind {
            StatementKind::Assign(lhs, rvalue) => (lhs, rvalue, false),
            StatementKind::Reassign(lhs, rvalue) => (lhs, rvalue, true),
            _ => return false,
        };
        let mut changed = false;

        // 1. If we are copying a managed value, we must increment its reference count.
        if let Some(place) = get_copy_source_place(rvalue) {
            if self.should_incref_source(ctx, &place, lhs, managed_locals) {
                new_stmts.push(Statement {
                    kind: StatementKind::IncRef(place),
                    span: stmt.span,
                });
                changed = true;
            }
        }
        // 2. Handle specialized coercion casts that might involve managed field projections.
        else if let Rvalue::Cast(operand, target_ty) = rvalue {
            if self.handle_cast(operand, target_ty, stmt.span, ctx, new_stmts) {
                changed = true;
            }
        }

        // 2b. Moving from a parameter creates a new managed local that will
        // eventually get DecRef'd at StorageDead.  Since the caller does NOT
        // IncRef before the call (borrow semantics), the move-from-param must
        // IncRef to keep the shared allocation alive.  Without this, the
        // StorageDead DecRef on the destination local would prematurely free
        // the caller's allocation.
        if let Some(param_place) = get_move_from_param_place(rvalue, ctx.arg_count) {
            if is_place_managed(
                &param_place,
                ctx.local_decls,
                ctx.auto_copy_types,
                ctx.field_types,
                ctx.type_params,
            ) {
                new_stmts.push(Statement {
                    kind: StatementKind::IncRef(param_place),
                    span: stmt.span,
                });
                changed = true;
            }
        }

        // 3. If we are creating a collection, increment the RC of every managed element.
        if let Rvalue::Aggregate(_, operands) = rvalue {
            if self.handle_aggregate(operands, stmt.span, ctx, new_stmts) {
                changed = true;
            }
        }

        // 4. If this is a re-assignment, we must decrement the RC of the OLD value.
        // We do this AFTER IncRefs to handle the case where we assign something to itself.
        if is_reassign && self.should_decref_reassign(ctx, lhs) {
            new_stmts.push(Statement {
                kind: StatementKind::DecRef(lhs.clone()),
                span: stmt.span,
            });
            changed = true;
        }

        changed
    }

    /// Handles a storage end-of-life by decrementing the RC of managed locals.
    fn handle_storage_dead(
        &self,
        stmt: &Statement,
        place: &Place,
        managed_locals: &std::collections::HashSet<crate::mir::Local>,
        new_stmts: &mut Vec<Statement>,
    ) -> bool {
        if managed_locals.contains(&place.local) {
            new_stmts.push(Statement {
                kind: StatementKind::DecRef(place.clone()),
                span: stmt.span,
            });
            return true;
        }
        false
    }

    /// Determines if a source value being copied needs an IncRef.
    fn should_incref_source(
        &self,
        ctx: &PerceusContext,
        source: &Place,
        dest: &Place,
        managed_locals: &std::collections::HashSet<crate::mir::Local>,
    ) -> bool {
        // Direct managed place?
        if is_place_managed(
            source,
            ctx.local_decls,
            ctx.auto_copy_types,
            ctx.field_types,
            ctx.type_params,
        ) {
            return true;
        }

        // Fallback for complex projections where the destination is definitely managed.
        source
            .projection
            .iter()
            .any(|e| matches!(e, PlaceElem::Field(_)))
            && managed_locals.contains(&dest.local)
    }

    /// Handles RC for operands inside an aggregate (like a List or Map).
    fn handle_aggregate(
        &self,
        operands: &[Operand],
        span: Span,
        ctx: &PerceusContext,
        new_stmts: &mut Vec<Statement>,
    ) -> bool {
        let mut changed = false;
        for op in operands {
            let place = match op {
                Operand::Copy(p) | Operand::Move(p) => Some(p),
                _ => None,
            };
            if let Some(place) = place {
                if is_place_managed(
                    place,
                    ctx.local_decls,
                    ctx.auto_copy_types,
                    ctx.field_types,
                    ctx.type_params,
                ) {
                    new_stmts.push(Statement {
                        kind: StatementKind::IncRef(place.clone()),
                        span,
                    });
                    changed = true;
                }
            }
        }
        changed
    }

    /// Handles RC for cast operations.
    fn handle_cast(
        &self,
        operand: &Operand,
        target_ty: &crate::ast::types::Type,
        span: Span,
        ctx: &PerceusContext,
        new_stmts: &mut Vec<Statement>,
    ) -> bool {
        if let Operand::Copy(place) = operand {
            if place
                .projection
                .iter()
                .any(|e| matches!(e, PlaceElem::Field(_)))
                && MirType::from_type_kind(&target_ty.kind)
                    .is_managed(ctx.auto_copy_types, ctx.type_params)
            {
                new_stmts.push(Statement {
                    kind: StatementKind::IncRef(place.clone()),
                    span,
                });
                return true;
            }
        }
        false
    }

    /// Determines if a reassignment destination needs a DecRef.
    fn should_decref_reassign(&self, ctx: &PerceusContext, lhs: &Place) -> bool {
        // Parameters (1..=arg_count) are caller-owned; callee must not DecRef them.
        lhs.local.0 > ctx.arg_count
            && is_place_managed(
                lhs,
                ctx.local_decls,
                ctx.auto_copy_types,
                ctx.field_types,
                ctx.type_params,
            )
    }
}

/// Extract the source place from a Copy or Ref rvalue (aliasing operations).
///
/// Move operands are excluded because they transfer ownership rather than
/// creating an alias — no IncRef is needed for a move.
fn get_copy_source_place(rvalue: &Rvalue) -> Option<Place> {
    match rvalue {
        Rvalue::Use(Operand::Copy(place)) => Some(place.clone()),
        Rvalue::Ref(place) => Some(place.clone()),
        _ => None,
    }
}

/// Extract the source place from a Move whose source is a function parameter.
///
/// When a callee moves from a parameter (e.g. `_4 = move _1 as String`),
/// it creates a new managed local that Perceus will DecRef at StorageDead.
/// Since callers use borrow semantics (no IncRef before the call), the
/// move must IncRef to prevent the StorageDead DecRef from prematurely
/// freeing the caller's allocation.
fn get_move_from_param_place(rvalue: &Rvalue, arg_count: usize) -> Option<Place> {
    let place = match rvalue {
        Rvalue::Use(Operand::Move(place)) => Some(place),
        Rvalue::Cast(operand, _) => match operand.as_ref() {
            Operand::Move(place) => Some(place),
            _ => None,
        },
        _ => None,
    }?;
    if place.local.0 >= 1 && place.local.0 <= arg_count {
        Some(place.clone())
    } else {
        None
    }
}

/// Computes whether a place represents a managed typed object, even through projections.
///
/// Handles `Index` projections for collection types and `Field` projections for:
/// - `Option<T>` — `Field(0)` yields the inner `T`
/// - `Tuple(T0, T1, ...)` — `Field(i)` yields `Ti`
/// - Custom struct/class types — `Field(i)` is resolved via `field_types`
///
/// Enum `Field(i)` projections cannot be resolved here (the field type depends on
/// which variant is active), so the Perceus main loop falls back to checking
/// `managed_locals.contains(&lhs.local)` for those cases.
///
/// Uses [`MirType`] throughout, which stores collection element types as resolved
/// `MirType` values (not `Box<Expression>`), so this function never needs to
/// inspect AST expression nodes.
fn is_place_managed(
    place: &Place,
    local_decls: &[crate::mir::LocalDecl],
    auto_copy_types: &std::collections::HashSet<String>,
    field_types: &std::collections::HashMap<String, Vec<crate::ast::types::Type>>,
    type_params: &std::collections::HashSet<String>,
) -> bool {
    // Start from the MIR-level resolved type of the base local.
    let mut current: MirType = local_decls[place.local.0].mir_ty.clone();

    for elem in &place.projection {
        let next = match elem {
            PlaceElem::Deref => return false,
            // For Index projections, extract the element type from the collection.
            // MirType stores element types as resolved MirType values — no AST
            // expression nodes to inspect.
            PlaceElem::Index(_) => match current {
                MirType::Array(elem) | MirType::List(elem) | MirType::Set(elem) => *elem,
                MirType::Map(_, v) => *v,
                _ => return false,
            },
            PlaceElem::Field(i) => match &current {
                // Option<T>.Field(0) → the inner type T
                MirType::Option(inner) if *i == 0 => *inner.clone(),
                // Tuple(T0, T1, …).Field(i) → Ti
                MirType::Tuple(elems) => match elems.get(*i).cloned() {
                    Some(t) => t,
                    None => return false,
                },
                // Custom(struct/class).Field(i) → look up in the pre-built field_types map
                // and convert to MirType on the fly.
                // Resolving here (not at LocalDecl creation time) avoids needing a separate
                // MIR-level field_types map — we reuse the existing one that holds `Type`.
                MirType::Custom(name) => {
                    match field_types.get(name.as_str()).and_then(|fs| fs.get(*i)) {
                        Some(ty) => MirType::from_type_kind(&ty.kind),
                        None => return false,
                    }
                }
                _ => return false,
            },
        };
        current = next;
    }

    current.is_managed(auto_copy_types, type_params)
}
