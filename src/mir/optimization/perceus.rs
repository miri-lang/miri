// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Perceus: Precise Reference Counting and Reuse.
//!
//! This pass inserts `IncRef` and `DecRef` operations for managed (heap-allocated)
//! types such as `String`, `List`, `Map`, `Set`, and user-defined types.
//! It implements the "Functional But In-Place" (FBIP) strategy where possible.

use crate::ast::types::{Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::block::BasicBlockData;
use crate::mir::optimization::OptimizationPass;
use crate::mir::place_type::PlaceTypes;
use crate::mir::rc::RcClassifier;
use crate::mir::statement::{Statement, StatementKind};
use crate::mir::terminator::{arg_mode, ArgMode, TerminatorKind};
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
pub(super) struct PerceusContext<'a> {
    pub(super) local_decls: &'a [crate::mir::LocalDecl],
    pub(super) rc_classifier: &'a RcClassifier,
    field_types: &'a std::collections::HashMap<String, Vec<crate::ast::types::Type>>,
    class_type_params: &'a std::collections::HashMap<String, Vec<String>>,
    type_params: &'a std::collections::HashSet<String>,
    /// The parameters the body leaves open; see [`Body::open_params`].
    open_params: &'a std::collections::HashSet<String>,
    /// Maps each closure local to the ordered AST types of its captured variables.
    /// Used by `is_place_managed` to resolve `closure.Field(i)` projections so that
    /// `handle_aggregate` can IncRef managed captures when building a closure aggregate.
    closure_capture_types:
        &'a std::collections::HashMap<crate::mir::Local, Vec<crate::ast::types::Type>>,
    borrowed: &'a BorrowedLocals,
}

impl<'a> PerceusContext<'a> {
    /// The context of `body`, reading `borrowed` for what it does not own.
    fn of(body: &'a Body, borrowed: &'a BorrowedLocals) -> Self {
        PerceusContext {
            local_decls: &body.local_decls,
            rc_classifier: &body.rc_classifier,
            field_types: &body.field_types,
            class_type_params: &body.class_type_params,
            type_params: &body.type_params,
            open_params: &body.open_params,
            closure_capture_types: &body.closure_capture_types,
            borrowed,
        }
    }

    /// Whether a value of `kind`, written in the body, is reference counted.
    fn is_counted(&self, kind: &TypeKind) -> bool {
        self.rc_classifier.is_counted(kind, self.type_params)
    }

    /// Whether a value of `kind` is held as its bytes, with no count.
    fn holds_value(&self, kind: &TypeKind) -> bool {
        self.rc_classifier
            .value_layout(kind, self.type_params)
            .is_some()
    }
}

/// The locals a body reads but does not own, so never releases: its borrowed
/// parameters, which the caller owns, and the closure captures it only reads,
/// which the closure's environment owns and its destructor releases.
///
/// A parameter the body takes over (`Body::owned_params`) is not borrowed: its
/// binding moves the caller's reference out into a local the body releases.
///
/// A capture the body assigns is not borrowed. Its value is replaced by one the
/// body owns, so it takes its own reference on entry and is then released like
/// any other local — see [`Perceus::retain_written_captures`].
///
/// A local that reads a value out of a place the body owns, while nothing
/// changes that place, borrows it from that place the way a parameter borrows
/// from its caller (see [`super::local_borrows`]).
struct BorrowedLocals {
    arg_count: usize,
    read_only_captures: std::collections::HashSet<crate::mir::Local>,
    /// The `out` parameters of a host body, whose value goes back to the
    /// caller through a slot. A new value assigned to one takes the place of
    /// the caller's, so the old one is released; the parameter is otherwise
    /// borrowed, and never released at the body's end.
    written_back: std::collections::HashSet<crate::mir::Local>,
    /// The parameters the body takes over, in parameter order.
    owned_params: Vec<bool>,
    /// The locals that borrow from a place the body owns.
    reads: std::collections::HashSet<crate::mir::Local>,
}

/// The locals `body` assigns, or makes a direct call's destination: the
/// writes that decide whether a capture is read-only. Narrower than
/// [`Body::written_locals`], which also counts `out` arguments and the
/// destinations of virtual calls and kernel launches.
///
/// TODO: a capture written only through an `out` argument still counts as
/// read-only here, so the value the callee stores into it is released twice —
/// the heap guard reports a double free for a closure that passes a captured
/// list to an `out` parameter and reads it back. Deciding read-only captures
/// from `Body::written_locals` releases it once.
fn assigned_or_call_written_locals(body: &Body) -> std::collections::HashSet<crate::mir::Local> {
    let call_destinations = body
        .basic_blocks
        .iter()
        .filter_map(|block| block.terminator.as_ref())
        .filter_map(|terminator| match &terminator.kind {
            TerminatorKind::Call { destination, .. } => Some(destination),
            TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable
            | TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::VirtualCall { .. } => None,
        });
    body.basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(Statement::written_place)
        .chain(call_destinations)
        .map(|place| place.local)
        .collect()
}

impl BorrowedLocals {
    fn of(body: &Body) -> Self {
        let written = assigned_or_call_written_locals(body);
        BorrowedLocals {
            arg_count: body.arg_count,
            owned_params: body.owned_params.clone(),
            read_only_captures: body
                .env_capture_locals
                .iter()
                .copied()
                .filter(|local| !written.contains(local))
                .collect(),
            written_back: crate::mir::body::written_back_out_params(body),
            reads: std::collections::HashSet::new(),
        }
    }

    fn contains(&self, local: crate::mir::Local) -> bool {
        let is_borrowed_param = (1..=self.arg_count).contains(&local.0)
            && !self.owned_params.get(local.0 - 1).copied().unwrap_or(false);
        is_borrowed_param || self.read_only_captures.contains(&local) || self.reads.contains(&local)
    }
}

impl OptimizationPass for Perceus {
    fn run(&mut self, body: &mut Body) -> bool {
        // Step 0: a lent argument held only through a class field gets a local
        // of its own across a call that may release it, retained below like
        // any other copy of a counted field.
        let held = super::lent_arguments::hold_lent_field_arguments(body);

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
        // managed values are released by the runtime destructor (`miri.$dtor.{lambda}`)
        // stored in the closure struct.  This works for both local and cross-function
        // closures and eliminates the double-free that would occur if both the
        // destructor and Perceus decremented the same capture on drop.
        let mut borrowed = BorrowedLocals::of(body);
        let mut managed_locals = self.identify_managed_locals(body, &borrowed);
        let borrows = super::local_borrows::infer_local_borrows(
            body,
            &PerceusContext::of(body, &borrowed),
            &managed_locals,
        );
        for local in borrows.keys() {
            managed_locals.remove(local);
            borrowed.reads.insert(*local);
        }
        body.borrowed_locals = borrows;
        let mut changed = self.retain_written_captures(body, &managed_locals) || held;

        // Step 2: Iterate through every block of code and inject RC instructions.

        // Split the borrow: we need mutable access to basic_blocks, but only
        // immutable access to the rest of the metadata.
        let Body {
            ref mut basic_blocks,
            ref local_decls,
            ref rc_classifier,
            ref field_types,
            ref class_type_params,
            ref type_params,
            ref open_params,
            ref closure_capture_types,
            ..
        } = *body;

        let ctx = PerceusContext {
            local_decls,
            rc_classifier,
            field_types,
            class_type_params,
            type_params,
            open_params,
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
    /// without reference counting). An owned parameter is excluded too: its one
    /// reference leaves through the move that binds it, and the local it moves
    /// into is the one released.
    fn identify_managed_locals(
        &self,
        body: &Body,
        borrowed: &BorrowedLocals,
    ) -> std::collections::HashSet<crate::mir::Local> {
        (1..body.local_decls.len())
            .map(crate::mir::Local)
            .filter(|&local| {
                !borrowed.contains(local)
                    && !body.is_owned_param(local)
                    && body.is_counted_local(local)
            })
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
        if fund_owned_arguments(block, &mut new_stmts) {
            changed = true;
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

        // 1. If we are copying a managed value, we must increment its reference
        // count — unless the destination borrows it from the place it reads.
        if let Some(place) = get_copy_source_place(rvalue) {
            let borrows = lhs.projection.is_empty() && ctx.borrowed.reads.contains(&lhs.local);
            if !borrows && self.should_incref_source(ctx, &place, lhs, managed_locals) {
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
    /// destructor (`miri.$dtor.{lambda_name}`) when the closure RC reaches zero, so
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
    pub(super) fn should_incref_source(
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

        // A value held as its bytes read from a counted one — an optional
        // read out of the placeholder word the absent value is spelled at —
        // copies what it reads and holds no reference to it.
        if place_declared_type(dest, ctx).is_some_and(|declared| ctx.holds_value(&declared.kind)) {
            return false;
        }

        // Direct managed place?
        if is_place_managed(source, ctx) {
            return true;
        }

        // A value held as its bytes — in a class field, a struct, an element —
        // is copied, never retained.
        if place_declared_type(source, ctx).is_some_and(|declared| ctx.holds_value(&declared.kind))
        {
            return false;
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
        for place in operands.iter().filter_map(Operand::place) {
            if is_place_managed(place, ctx) {
                new_stmts.push(Statement {
                    kind: StatementKind::IncRef(place.clone()),
                    span,
                });
                changed = true;
            }
        }
        changed
    }

    /// Handles RC for cast operations.
    ///
    /// A cast that reads its source by copy — a class value handed on at a
    /// trait or abstract type, or a managed field read out of an aggregate —
    /// leaves the source's holder and the destination each owning the value, so
    /// the source is retained exactly as a plain copy is. A cast that moves its
    /// source transfers the reference it held.
    fn handle_cast(
        &self,
        operand: &Operand,
        target_ty: &crate::ast::types::Type,
        span: Span,
        ctx: &PerceusContext,
        new_stmts: &mut Vec<Statement>,
    ) -> bool {
        if let Operand::Copy(place) = operand {
            let aliases_managed_source = is_place_managed(place, ctx)
                || place
                    .projection
                    .iter()
                    .any(|e| matches!(e, PlaceElem::Field(_)));
            if aliases_managed_source && ctx.is_counted(&target_ty.kind) {
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
    ///
    /// A borrowed local's own value belongs to the caller or the closure
    /// environment, so overwriting the local must not release it. What is
    /// reached *through* a projection of it — `self.value` inside a method — is
    /// owned by the object the local points at, and overwriting it releases the
    /// value it replaces like any other store.
    fn should_decref_reassign(&self, ctx: &PerceusContext, lhs: &Place) -> bool {
        let overwrites_borrowed_value = lhs.projection.is_empty()
            && ctx.borrowed.contains(lhs.local)
            && !ctx.borrowed.written_back.contains(&lhs.local);
        lhs.local.0 != 0 && !overwrites_borrowed_value && is_place_managed(lhs, ctx)
    }
}

/// Retain, at the end of the block, each argument its terminating call passes
/// [`ArgMode::Owned`]: the callee releases the reference it is handed, so the
/// caller pays for one.
///
/// A constant argument needs nothing: every constant that reaches a managed
/// parameter is inert to reference counting. A string literal is immortal, so
/// the callee's release of it does nothing; `None` is the null optional, which
/// a release skips; and a regex literal never reaches MIR as a constant —
/// lowering turns it into the call that builds the pattern.
///
/// The retain is unconditional on the argument's place: the callee's parameter
/// is a managed type, which is what made it ownable, so the value is a counted
/// reference even where the walk over a projection cannot resolve its type.
fn fund_owned_arguments(block: &BasicBlockData, new_stmts: &mut Vec<Statement>) -> bool {
    let Some(terminator) = &block.terminator else {
        return false;
    };
    let TerminatorKind::Call {
        args, arg_modes, ..
    } = &terminator.kind
    else {
        return false;
    };
    let before = new_stmts.len();
    for (position, arg) in args.iter().enumerate() {
        if arg_mode(arg_modes, position) != ArgMode::Owned {
            continue;
        }
        if let Operand::Copy(place) | Operand::Move(place) = arg {
            new_stmts.push(Statement {
                kind: StatementKind::IncRef(place.clone()),
                span: terminator.span,
            });
        }
    }
    new_stmts.len() > before
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
/// The walk decides which projection applies over [`MirType`], which stores
/// collection element types resolved. Alongside it the declared type is
/// carried through every projection, because `MirType::Custom` names a class
/// without its type arguments; whether the place is counted is asked of that
/// declared type.
fn is_place_managed(place: &Place, ctx: &PerceusContext) -> bool {
    place_declared_type(place, ctx).is_some_and(|declared| ctx.is_counted(&declared.kind))
}

/// The declared type of what `place` reaches; see [`PlaceTypes::declared`].
pub(super) fn place_declared_type(place: &Place, ctx: &PerceusContext) -> Option<Type> {
    PlaceTypes {
        local_decls: ctx.local_decls,
        field_types: ctx.field_types,
        class_type_params: ctx.class_type_params,
        closure_capture_types: ctx.closure_capture_types,
        definitions: ctx.rc_classifier.definitions(),
        type_params: ctx.type_params,
        open_params: ctx.open_params,
        strict: true,
    }
    .declared(place)
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
        MirType::Array(elem) | MirType::List(elem) => elem.is_inline_vector(),
        _ => false,
    }
}
