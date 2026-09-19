// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Perceus: Precise Reference Counting and Reuse.
//!
//! This pass inserts `IncRef` and `DecRef` operations for managed (heap-allocated)
//! types such as `String`, `List`, `Map`, `Set`, and user-defined types.
//! It implements the "Functional But In-Place" (FBIP) strategy where possible.

use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::block::BasicBlockData;
use crate::mir::lowering::{field_type_in_instance, type_argument};
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
    unmanaged_type_names: &'a std::collections::HashSet<String>,
    field_types: &'a std::collections::HashMap<String, Vec<crate::ast::types::Type>>,
    class_type_params: &'a std::collections::HashMap<String, Vec<String>>,
    type_params: &'a std::collections::HashSet<String>,
    /// Maps each closure local to the ordered AST types of its captured variables.
    /// Used by `is_place_managed` to resolve `closure.Field(i)` projections so that
    /// `handle_aggregate` can IncRef managed captures when building a closure aggregate.
    closure_capture_types:
        &'a std::collections::HashMap<crate::mir::Local, Vec<crate::ast::types::Type>>,
    borrowed: &'a BorrowedLocals,
}

/// The locals a body reads but does not own, so never releases: its parameters,
/// which the caller owns, and the closure captures it only reads, which the
/// closure's environment owns and its destructor releases.
///
/// A capture the body assigns is not borrowed. Its value is replaced by one the
/// body owns, so it takes its own reference on entry and is then released like
/// any other local — see [`Perceus::retain_written_captures`].
struct BorrowedLocals {
    arg_count: usize,
    read_only_captures: std::collections::HashSet<crate::mir::Local>,
}

impl BorrowedLocals {
    fn of(body: &Body) -> Self {
        let written = body.written_locals();
        BorrowedLocals {
            arg_count: body.arg_count,
            read_only_captures: body
                .env_capture_locals
                .iter()
                .copied()
                .filter(|local| !written.contains(local))
                .collect(),
        }
    }

    fn contains(&self, local: crate::mir::Local) -> bool {
        (1..=self.arg_count).contains(&local.0) || self.read_only_captures.contains(&local)
    }
}

impl OptimizationPass for Perceus {
    fn run(&mut self, body: &mut Body) -> bool {
        // Step 1: Identify which variables actually need tracking (managed locals).
        //
        // NOTE: We do NOT exit early when managed_locals is empty. Even when there
        // are no non-parameter managed locals, a closure aggregate may capture a
        // managed parameter (e.g. `fn make_counter(items [int]) fn() int`). In that
        // case `handle_aggregate` must still IncRef the parameter; it uses
        // `is_place_managed` rather than `managed_locals`, so it works correctly.
        // Exiting early when managed_locals is empty would skip that IncRef, causing
        // the parameter's RC to remain at 1 while the closure holds a dangling ref.
        //
        // Per-capture DecRef at StorageDead is intentionally OMITTED: captured
        // managed values are released by the runtime destructor (`__dtor_{lambda}`)
        // stored in the closure struct.  This works for both local and cross-function
        // closures and eliminates the double-free that would occur if both the
        // destructor and Perceus decremented the same capture on drop.
        let borrowed = BorrowedLocals::of(body);
        let managed_locals = self.identify_managed_locals(body, &borrowed);
        let mut changed = self.retain_written_captures(body, &managed_locals);

        // Step 2: Iterate through every block of code and inject RC instructions.

        // Split the borrow: we need mutable access to basic_blocks, but only
        // immutable access to the rest of the metadata.
        let Body {
            ref mut basic_blocks,
            ref local_decls,
            ref unmanaged_type_names,
            ref field_types,
            ref class_type_params,
            ref type_params,
            ref closure_capture_types,
            ..
        } = *body;

        let ctx = PerceusContext {
            local_decls,
            unmanaged_type_names,
            field_types,
            class_type_params,
            type_params,
            closure_capture_types,
            borrowed: &borrowed,
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
    /// Excludes borrowed locals — parameters and closure captures — and
    /// "Auto-copy" types (which are small enough to be copied byte-for-byte
    /// without reference counting).
    fn identify_managed_locals(
        &self,
        body: &Body,
        borrowed: &BorrowedLocals,
    ) -> std::collections::HashSet<crate::mir::Local> {
        body.local_decls
            .iter()
            .enumerate()
            .filter(|(i, decl)| {
                *i != 0
                    && !borrowed.contains(crate::mir::Local(*i))
                    && decl
                        .mir_ty
                        .is_managed(&body.unmanaged_type_names, &body.type_params)
            })
            .map(|(i, _)| crate::mir::Local(i))
            .collect()
    }

    /// Give each managed capture the body assigns its own reference on entry,
    /// so the body owns the value it later overwrites or drops rather than
    /// releasing the one its closure's environment holds.
    fn retain_written_captures(
        &self,
        body: &mut Body,
        managed_locals: &std::collections::HashSet<crate::mir::Local>,
    ) -> bool {
        let Some(entry) = body.basic_blocks.first_mut() else {
            return false;
        };
        let retains: Vec<Statement> = body
            .env_capture_locals
            .iter()
            .filter(|local| managed_locals.contains(local))
            .map(|&local| Statement {
                kind: StatementKind::IncRef(Place::new(local)),
                span: body.span,
            })
            .collect();
        let changed = !retains.is_empty();
        entry.statements.splice(0..0, retains);
        changed
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
                self.handle_storage_dead(ctx, stmt, place, managed_locals, new_stmts)
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

        // 2b. A move out of a parameter is retained so that the release the
        // destination eventually performs balances it.
        if let Some(param_place) = self.move_from_param_to_retain(ctx, rvalue, lhs) {
            new_stmts.push(Statement {
                kind: StatementKind::IncRef(param_place),
                span: stmt.span,
            });
            changed = true;
        }

        // 3. If we are creating a collection or calling a math intrinsic,
        // increment the RC of every managed element/operand.
        if let Rvalue::Aggregate(_, operands) | Rvalue::MathIntrinsic(_, operands) = rvalue {
            if self.handle_aggregate(operands, stmt.span, ctx, lhs, new_stmts) {
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
    ///
    /// Managed captures inside closure locals are now DecRef'd by the runtime
    /// destructor (`__dtor_{lambda_name}`) when the closure RC reaches zero, so
    /// Perceus no longer needs to emit per-field DecRefs for closure locals.
    fn handle_storage_dead(
        &self,
        _ctx: &PerceusContext,
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

    /// The parameter that a move-from-parameter assignment must retain, if any.
    ///
    /// A caller does not IncRef before a call (borrow semantics), so moving a
    /// parameter into a local that Perceus later DecRefs at its `StorageDead`
    /// must IncRef first, or that DecRef frees the caller's allocation while the
    /// caller still holds it.
    ///
    /// The retain is only correct when the destination is itself managed. A
    /// destination that is not — a cast to `Self`, to a generic parameter, or to
    /// a raw pointer — is never DecRef'd, because both the `StorageDead` release
    /// and this retain are decided by the same managed-type predicate. Retaining
    /// into one of those strands the reference and leaks the parameter's value.
    fn move_from_param_to_retain(
        &self,
        ctx: &PerceusContext,
        rvalue: &Rvalue,
        lhs: &Place,
    ) -> Option<Place> {
        let param_place = get_move_from_borrowed_place(rvalue, ctx.borrowed)?;
        let is_managed = |place: &Place| is_place_managed(place, ctx);
        (is_managed(&param_place) && is_managed(lhs)).then_some(param_place)
    }

    /// Determines if a source value being copied needs an IncRef.
    fn should_incref_source(
        &self,
        ctx: &PerceusContext,
        source: &Place,
        dest: &Place,
        managed_locals: &std::collections::HashSet<crate::mir::Local>,
    ) -> bool {
        // An inline element slot copies the source's bytes and keeps no pointer
        // to it, so the write takes no reference.
        if writes_inline_element(ctx, dest) {
            return false;
        }

        // Direct managed place?
        if is_place_managed(source, ctx) {
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
    ///
    /// A container that stores its elements inline copies the operand's bytes
    /// instead of keeping its pointer, so it takes no reference and the operand
    /// must not be retained — the temporary that built the element is then freed
    /// at its own `StorageDead`.
    fn handle_aggregate(
        &self,
        operands: &[Operand],
        span: Span,
        ctx: &PerceusContext,
        lhs: &Place,
        new_stmts: &mut Vec<Statement>,
    ) -> bool {
        if stores_elements_inline(ctx, lhs) {
            return false;
        }
        let mut changed = false;
        for op in operands {
            let place = match op {
                Operand::Copy(p) | Operand::Move(p) => Some(p),
                _ => None,
            };
            if let Some(place) = place {
                if is_place_managed(place, ctx) {
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
                    .is_managed(ctx.unmanaged_type_names, ctx.type_params)
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
    // TODO: the borrowed test ignores the projection, so `self.field = x` inside
    // a method is rejected — `self` is a parameter — and the value the field held
    // is never released, leaking one allocation per store. What a field holds is
    // owned by the object, not by the caller, so the guard belongs only on an
    // unprojected destination. Widening it reaches every method that overwrites a
    // field and every write through a captured value, so it needs the whole suite
    // under the heap guard, not the leak check alone.
    fn should_decref_reassign(&self, ctx: &PerceusContext, lhs: &Place) -> bool {
        // A borrowed local's value belongs to the caller or the closure
        // environment; overwriting it must not release that value.
        lhs.local.0 != 0 && !ctx.borrowed.contains(lhs.local) && is_place_managed(lhs, ctx)
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

/// Extract the source place from a Move whose source is a borrowed local.
///
/// When a callee moves from a parameter (e.g. `_4 = move _1 as String`),
/// it creates a new managed local that Perceus will DecRef at StorageDead.
/// Since callers use borrow semantics (no IncRef before the call), the
/// move must IncRef to prevent the StorageDead DecRef from prematurely
/// freeing the caller's allocation. A closure capture is borrowed from the
/// closure's environment the same way.
fn get_move_from_borrowed_place(rvalue: &Rvalue, borrowed: &BorrowedLocals) -> Option<Place> {
    let place = match rvalue {
        Rvalue::Use(Operand::Move(place)) => Some(place),
        Rvalue::Cast(operand, _) => match operand.as_ref() {
            Operand::Move(place) => Some(place),
            _ => None,
        },
        _ => None,
    }?;
    if borrowed.contains(place.local) {
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
/// - Custom struct/class types — `Field(i)` is resolved via `field_types`, at the
///   type arguments of the instance it is projected from (see
///   [`field_type_in_instance`])
/// - Closure locals — `Field(i)` yields the type of captured variable `i`,
///   looked up from `closure_capture_types` using the root local index
///
/// Enum `Field(i)` projections cannot be resolved here (the field type depends on
/// which variant is active), so the Perceus main loop falls back to checking
/// `managed_locals.contains(&lhs.local)` for those cases.
///
/// The walk is over [`MirType`], which stores collection element types as
/// resolved `MirType` values. Alongside it the declared type is carried as far
/// as a class field or an option payload preserves it, because `MirType::Custom`
/// names a class without its type arguments.
fn is_place_managed(place: &Place, ctx: &PerceusContext) -> bool {
    let decl = &ctx.local_decls[place.local.0];
    let mut current: MirType = decl.mir_ty.clone();
    let mut declared: Option<Type> = Some(decl.ty.clone());

    for elem in &place.projection {
        let (next, next_declared) = match elem {
            PlaceElem::Deref => return false,
            // For Index projections, extract the element type from the collection.
            PlaceElem::Index(_) => {
                let element = match current {
                    MirType::Array(elem) | MirType::List(elem) | MirType::Set(elem) => *elem,
                    MirType::Map(_, v) => *v,
                    _ => return false,
                };
                // A vector element lives inline in the collection's buffer, so
                // indexing yields an interior address rather than a pointer to
                // an allocation of its own. Retaining or releasing it would
                // reach into the buffer's bytes.
                if is_inline_vector(&element) {
                    return false;
                }
                let element_declared = declared.as_ref().and_then(indexed_element_type);
                (element, element_declared)
            }
            PlaceElem::Field(i) => match project_field(ctx, place, &current, declared, *i) {
                Some(projected) => projected,
                None => return false,
            },
        };
        current = next;
        declared = next_declared;
    }

    current.is_managed(ctx.unmanaged_type_names, ctx.type_params)
}

/// The type a `Field(index)` projection yields from a value of type `current`,
/// paired with its declared type where the projection preserves it; `None` when
/// the field cannot be resolved.
fn project_field(
    ctx: &PerceusContext,
    place: &Place,
    current: &MirType,
    declared: Option<Type>,
    index: usize,
) -> Option<(MirType, Option<Type>)> {
    match current {
        // Option<T>.Field(0) → the inner type T
        MirType::Option(inner) if index == 0 => {
            let payload = declared.and_then(|ty| {
                let TypeKind::Option(inner) = ty.kind else {
                    return None;
                };
                Some(*inner)
            });
            Some((*inner.clone(), payload))
        }
        // Tuple(T0, T1, …).Field(i) → Ti
        MirType::Tuple(elems) => Some((elems.get(index)?.clone(), None)),
        // Custom(struct/class).Field(i) → the declared field type, read at the
        // instance's type arguments.
        MirType::Custom(name) => {
            let declared_field = ctx.field_types.get(name.as_str())?.get(index)?;
            let field_ty = field_type_in_instance(
                ctx.class_type_params,
                name,
                declared.as_ref(),
                declared_field,
            );
            Some((MirType::from_type_kind(&field_ty.kind), Some(field_ty)))
        }
        // Closure.Field(i) → the type of captured variable i.
        // Capture types are indexed by the root local because a closure
        // is always a single-level projection (never nested).
        MirType::Function => {
            let capture_ty = ctx.closure_capture_types.get(&place.local)?.get(index)?;
            Some((MirType::from_type_kind(&capture_ty.kind), None))
        }
        _ => None,
    }
}

/// The declared type an `Index` projection yields from a collection.
///
/// The walk's `MirType` keeps a collection's element resolved, but only the
/// declared spelling still carries a class element's type arguments — a
/// `List<Tagged<String>>` indexes to `Tagged<String>`, where the `MirType` is
/// `Custom("Tagged")` and names the class alone. Carrying the argument through
/// is what lets a field declared at the class parameter be read at the type it
/// actually holds. Indexing a sequence or a set yields its element; indexing a
/// map yields its value.
fn indexed_element_type(collection: &Type) -> Option<Type> {
    let TypeKind::Custom(name, Some(args)) = &collection.kind else {
        return None;
    };
    let argument = match BuiltinCollectionKind::from_name(name)? {
        BuiltinCollectionKind::Array | BuiltinCollectionKind::List | BuiltinCollectionKind::Set => {
            args.first()?
        }
        BuiltinCollectionKind::Map => args.get(1)?,
    };
    type_argument(argument)
}

/// Whether the destination of an aggregate is an array or list whose elements
/// are stored inline, laid out at their std430 stride rather than as pointers.
fn stores_elements_inline(ctx: &PerceusContext, lhs: &Place) -> bool {
    if !lhs.projection.is_empty() {
        return false;
    }
    holds_inline_elements(&ctx.local_decls[lhs.local.0].mir_ty)
}

/// Whether `dest` names one inline element of an array or list, as `arr[i]`
/// does for a vector element.
fn writes_inline_element(ctx: &PerceusContext, dest: &Place) -> bool {
    matches!(dest.projection.last(), Some(PlaceElem::Index(_)))
        && holds_inline_elements(&ctx.local_decls[dest.local.0].mir_ty)
}

/// Whether `ty` is a collection that lays its elements out inline.
fn holds_inline_elements(ty: &MirType) -> bool {
    match ty {
        MirType::Array(elem) | MirType::List(elem) => is_inline_vector(elem),
        _ => false,
    }
}

/// Whether `ty` is a vector type. A vector is stored inline wherever a
/// collection lays its elements out at their std430 stride, so it is the one
/// managed type whose element position holds bytes rather than a pointer.
///
/// The name alone decides it, which is sound only because every vector that
/// reaches here has a component with an inline layout: the type checker refuses
/// the rest where they are written (`validate_vector_component`), so a vector
/// codegen would have stored as a pointer never gets this far.
fn is_inline_vector(ty: &MirType) -> bool {
    matches!(ty, MirType::Custom(name) if crate::ast::types::vec_dim(name.as_str()).is_some())
}
