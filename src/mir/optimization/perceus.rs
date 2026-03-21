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
                *i > body.arg_count && is_managed_type(&decl.ty.kind, &body.auto_copy_types)
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

            // Pre-scan: collect managed locals that have a StorageDead in this block.
            // Used by the field-projection Use fix to avoid emitting IncRef for
            // temporary locals that have no balancing DecRef (e.g. `.length()` object
            // temps that are never StorageDead'd because obj_op_is_copy = false).
            let locals_with_dead_in_block: std::collections::HashSet<crate::mir::Local> = old_stmts
                .iter()
                .filter_map(|s| {
                    if let StatementKind::StorageDead(p) = &s.kind {
                        if managed_locals.contains(&p.local) {
                            Some(p.local)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();

            // Use a peekable iterator so we can look one statement ahead.
            // This is needed to avoid double-IncRef when bind_pattern already emits
            // an explicit IncRef(lhs) immediately after a field-projection assignment.
            let mut stmts_iter = old_stmts.into_iter().peekable();

            while let Some(stmt) = stmts_iter.next() {
                match &stmt.kind {
                    StatementKind::Assign(lhs, rvalue) => {
                        // Insert IncRef for Copy operands (aliasing).
                        // Move operands transfer ownership — no IncRef needed
                        // because the source gives up its reference.
                        if let Some(place) = get_copy_source_place(rvalue) {
                            if is_place_managed(&place, &body.local_decls, &body.auto_copy_types) {
                                new_stmts.push(Statement {
                                    kind: StatementKind::IncRef(place),
                                    span: stmt.span,
                                });
                                block_changed = true;
                            } else if place
                                .projection
                                .iter()
                                .any(|e| matches!(e, PlaceElem::Field(_)))
                                && locals_with_dead_in_block.contains(&lhs.local)
                            {
                                // Field-projected Use(Copy(...)): op.ty() returns the base
                                // local's type, not the field's type, so is_place_managed
                                // returns false. Use the pre-scanned set of locals that
                                // have a StorageDead in this block as a proxy: if the LHS
                                // has a StorageDead here, Perceus will emit DecRef(lhs),
                                // so we need a balancing IncRef for the field copy.
                                //
                                // Locals without StorageDead (e.g. .length() object temps
                                // where obj_op_is_copy=false skips emit_temp_drop) must
                                // NOT get an IncRef — there would be no matching DecRef.
                                //
                                // Guard: skip when the very next statement is IncRef(lhs) —
                                // that means an earlier lowering pass (e.g. bind_pattern)
                                // already emitted an explicit IncRef to balance DecRef(lhs)
                                // at StorageDead. Emitting a second IncRef here would
                                // double-increment the RC and cause a leak.
                                let already_incref = matches!(
                                    stmts_iter.peek(),
                                    Some(Statement {
                                        kind: StatementKind::IncRef(p),
                                        ..
                                    }) if p.local == lhs.local && p.projection.is_empty()
                                );
                                if !already_incref {
                                    new_stmts.push(Statement {
                                        kind: StatementKind::IncRef(place),
                                        span: stmt.span,
                                    });
                                    block_changed = true;
                                }
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
                                    && is_managed_type(&target_ty.kind, &body.auto_copy_types)
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

/// Returns true if a type is managed (heap-allocated, needs RC).
///
/// Managed types are: String, List, Array, Map, Set, and Custom types that
/// are NOT in the auto-copy set.
fn is_managed_type(kind: &TypeKind, auto_copy_types: &std::collections::HashSet<String>) -> bool {
    match kind {
        // Collections, Options, and Tuples use heap allocation and need RC.
        TypeKind::Option(_)
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Tuple(_) => true,
        // Note: String is excluded — it uses Box allocation, not alloc_with_rc,
        // so it doesn't have the [RC][payload] layout that IncRef/DecRef expect.
        TypeKind::Custom(name, _) => {
            // Exclude generic placeholders (Self, T, K, V, U) — they appear in
            // stdlib method bodies and represent unresolved types, not concrete
            // heap objects. Also exclude unresolved collection class names
            // (Array, List, Map, Set) that appear in stdlib method local_decls —
            // their locals may actually hold element values, not collections.
            // Auto-copy types use bitwise copy, no RC.
            !auto_copy_types.contains(name)
                && !matches!(
                    name.as_str(),
                    "Self" | "T" | "K" | "V" | "U" | "Array" | "List" | "Map" | "Set"
                )
        }
        _ => false,
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
fn is_place_managed(
    place: &Place,
    local_decls: &[crate::mir::LocalDecl],
    auto_copy_types: &std::collections::HashSet<String>,
) -> bool {
    let mut current = &local_decls[place.local.0].ty.kind;

    for elem in &place.projection {
        match elem {
            PlaceElem::Deref => {
                // Not supported statically in MIR without TypeChecker
                return false;
            }
            PlaceElem::Index(_) => match current {
                TypeKind::Array(inner, _) => match &inner.node {
                    crate::ast::expression::ExpressionKind::Type(ty, _) => {
                        current = &ty.kind;
                    }
                    crate::ast::expression::ExpressionKind::Identifier(name, _) => {
                        if matches!(name.as_str(), "String" | "List" | "Array" | "Map" | "Set")
                            || (!auto_copy_types.contains(name)
                                && !matches!(name.as_str(), "Self" | "T" | "K" | "V" | "U"))
                        {
                            return true;
                        }
                        return false;
                    }
                    _ => return false,
                },
                TypeKind::List(inner) | TypeKind::Set(inner) => match &inner.node {
                    crate::ast::expression::ExpressionKind::Type(ty, _) => {
                        current = &ty.kind;
                    }
                    crate::ast::expression::ExpressionKind::Identifier(name, _) => {
                        if matches!(name.as_str(), "String" | "List" | "Array" | "Map" | "Set")
                            || (!auto_copy_types.contains(name)
                                && !matches!(name.as_str(), "Self" | "T" | "K" | "V" | "U"))
                        {
                            return true;
                        }
                        return false;
                    }
                    _ => return false,
                },
                TypeKind::Map(_, v) => match &v.node {
                    crate::ast::expression::ExpressionKind::Type(ty, _) => {
                        current = &ty.kind;
                    }
                    crate::ast::expression::ExpressionKind::Identifier(name, _) => {
                        if matches!(name.as_str(), "String" | "List" | "Array" | "Map" | "Set")
                            || (!auto_copy_types.contains(name)
                                && !matches!(name.as_str(), "Self" | "T" | "K" | "V" | "U"))
                        {
                            return true;
                        }
                        return false;
                    }
                    _ => return false,
                },
                _ => return false,
            },
            PlaceElem::Field(_) => {
                // Requires struct type definitions. Skip for now.
                return false;
            }
        }
    }

    is_managed_type(current, auto_copy_types)
}
