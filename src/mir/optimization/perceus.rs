// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Perceus: Precise Reference Counting and Reuse.
//!
//! This pass inserts `IncRef` and `DecRef` operations for managed (heap-allocated)
//! types such as `String`, `List`, `Map`, `Set`, and user-defined types.
//! It implements the "Functional But In-Place" (FBIP) strategy where possible.

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

impl OptimizationPass for Perceus {
    fn run(&mut self, body: &mut Body) -> bool {
        let mut changed = false;

        // Pre-compute which locals are managed (heap-allocated, need RC) to avoid
        // borrow conflicts between iterating basic_blocks mutably and reading local_decls.
        // Auto-copy custom types (structs with only primitive fields, <= 128 bytes)
        // are excluded — they use bitwise copy and do not need RC.
        // Locals 1..=arg_count are function parameters (borrowed, caller owns RC).
        // Exclude them — only owned locals need IncRef/DecRef.
        let managed_locals: std::collections::HashSet<crate::mir::Local> = body
            .local_decls
            .iter()
            .enumerate()
            .filter(|(i, decl)| {
                *i > body.arg_count
                    && decl
                        .mir_ty
                        .is_managed(&body.auto_copy_types, &body.type_params)
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
                    StatementKind::Assign(lhs, rvalue) | StatementKind::Reassign(lhs, rvalue) => {
                        let is_reassign = matches!(stmt.kind, StatementKind::Reassign(_, _));

                        // Insert IncRef for Copy operands (aliasing).
                        // Move operands transfer ownership — no IncRef needed
                        // because the source gives up its reference.
                        if let Some(place) = get_copy_source_place(rvalue) {
                            if is_place_managed(
                                &place,
                                &body.local_decls,
                                &body.auto_copy_types,
                                &body.field_types,
                                &body.type_params,
                            ) {
                                new_stmts.push(Statement {
                                    kind: StatementKind::IncRef(place),
                                    span: stmt.span,
                                });
                                block_changed = true;
                            } else if place
                                .projection
                                .iter()
                                .any(|e| matches!(e, PlaceElem::Field(_)))
                                && managed_locals.contains(&lhs.local)
                            {
                                // Fallback for field projections whose type cannot be
                                // resolved statically by is_place_managed (e.g. enum
                                // variant fields where the layout depends on which variant
                                // is active). The LHS local IS managed, so Perceus will
                                // emit DecRef at its StorageDead — emit a balancing IncRef
                                // for the source field copy here.
                                new_stmts.push(Statement {
                                    kind: StatementKind::IncRef(place),
                                    span: stmt.span,
                                });
                                block_changed = true;
                            }
                        } else if let Rvalue::Cast(operand, target_ty) = rvalue {
                            // Handle Cast(Copy(place_with_field_projection), managed_ty):
                            // emitted by coerce_rvalue when op.ty() returns the base local
                            // type but the cast target is the field's actual managed type.
                            // Guard: only fire for field projections to avoid matching
                            // numeric casts (e.g. int→string) which have no field source.
                            if let Operand::Copy(place) = operand.as_ref() {
                                if place
                                    .projection
                                    .iter()
                                    .any(|e| matches!(e, PlaceElem::Field(_)))
                                    && MirType::from_type_kind(&target_ty.kind)
                                        .is_managed(&body.auto_copy_types, &body.type_params)
                                {
                                    new_stmts.push(Statement {
                                        kind: StatementKind::IncRef(place.clone()),
                                        span: stmt.span,
                                    });
                                    block_changed = true;
                                }
                            }
                        }
                        // Also insert IncRef for managed operands inside Aggregates.
                        // When a heap-allocated value is stored into a collection (Map,
                        // List, Array, Set), the collection holds an additional reference
                        // that must be reflected in the RC. This applies to both Copy
                        // and Move operands: Move operands need IncRef because Perceus
                        // will still insert DecRef at StorageDead for the source local.
                        if let Rvalue::Aggregate(_, operands) = rvalue {
                            for op in operands {
                                let place = match op {
                                    Operand::Copy(p) | Operand::Move(p) => Some(p),
                                    _ => None,
                                };
                                if let Some(place) = place {
                                    if is_place_managed(
                                        place,
                                        &body.local_decls,
                                        &body.auto_copy_types,
                                        &body.field_types,
                                        &body.type_params,
                                    ) {
                                        new_stmts.push(Statement {
                                            kind: StatementKind::IncRef(place.clone()),
                                            span: stmt.span,
                                        });
                                        block_changed = true;
                                    }
                                }
                            }
                        }
                        // For Reassign: the LHS already holds a live reference that must
                        // be released before the new value is written.  Emit DecRef(lhs)
                        // after any IncRef for the rhs (preserving alias-safe order) and
                        // before the Reassign statement itself.
                        if is_reassign
                            && is_place_managed(
                                lhs,
                                &body.local_decls,
                                &body.auto_copy_types,
                                &body.field_types,
                                &body.type_params,
                            )
                        {
                            new_stmts.push(Statement {
                                kind: StatementKind::DecRef(lhs.clone()),
                                span: stmt.span,
                            });
                            block_changed = true;
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
