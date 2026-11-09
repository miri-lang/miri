// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reference-count insertion: the retains and releases a body's counted values need.
//!
//! Miri manages memory with reference counting and inferred borrowing over
//! copy-on-write values. A call lends its arguments; only an argument the callee
//! keeps is counted again, and `out` is the only way a callee writes a caller's
//! value. A value whose release nothing can observe is released at its last use;
//! a value whose release may run a `drop` hook is released where it is consumed,
//! else at its owner's scope end in reverse declaration order. No pass invents a
//! retain, release or copy the source did not name; passes move, merge or delete
//! the ones it named.
//!
//! This pass writes those operations on the MIR once per host body. It inserts an
//! `IncRef` where a counted value gains a holder (an assignment that keeps it, a
//! field or element store, a closure capture, an argument a callee keeps) and a
//! `DecRef` where a holder goes away. Today every release of a local lands at
//! that local's `StorageDead`, which is its scope end: placing a hook-free value's
//! release at its last use instead needs a liveness-driven placement this pass
//! does not perform yet, so a hook-free value may be held past its last use until
//! that lands. The pass never reuses a released block for a new value.
//!
//! The pass does not decide ownership itself. It reads the answers of the passes
//! that own each question: [`super::owned_params`] (which parameters a callee
//! takes over from its callers, inferred over the whole program),
//! [`super::local_borrows`] (which locals read out of a place the body holds and
//! need no count of their own), and [`crate::mir::rc::RcClassifier`] (whether a
//! value, slot or element is counted at all). [`super::rc_elision`] runs after it
//! and cancels the pairs that have no net effect, and
//! [`super::unshare_elision`] removes uniqueness checks an earlier one answered.

use crate::ast::types::{Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::block::BasicBlockData;
use crate::mir::facts::OwnershipFacts;
use crate::mir::place_type::PlaceTypes;
use crate::mir::rc::RcClassifier;
use crate::mir::statement::{Statement, StatementKind};
use crate::mir::terminator::{arg_mode, ArgMode, TerminatorKind};
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
    instance: Option<&'a crate::ast::types::Type>,
    type_params: &'a std::collections::HashSet<String>,
    /// The parameters the body leaves open; see [`crate::mir::Generics::open_params`].
    open_params: &'a std::collections::HashSet<String>,
    /// Maps each closure local to the ordered AST types of its captured variables.
    /// Used by `place_is_counted` to resolve `closure.Field(i)` projections so that
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
            instance: body.generics.instance.as_ref(),
            type_params: &body.generics.type_params,
            open_params: &body.generics.open_params,
            closure_capture_types: &body.captures.closure_capture_types,
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
/// A parameter the body takes over (`OwnershipFacts::owned_params`) is not borrowed: its
/// binding moves the caller's reference out into a local the body releases.
///
/// A capture the body writes — assigns, makes a call's destination, or hands
/// to an `out` parameter — is not borrowed. Its value is replaced by one the
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

impl BorrowedLocals {
    fn of(body: &Body, ownership: &OwnershipFacts) -> Self {
        // Every write counts, an `out` argument's included: a callee that
        // replaces a capture handed to it releases the value it replaces, so
        // the body must own that value rather than borrow the environment's.
        let written = body.written_locals();
        BorrowedLocals {
            arg_count: body.arg_count,
            owned_params: ownership.owned_params.clone(),
            read_only_captures: body
                .captures
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

impl Perceus {
    /// Inserts every retain and release into `body`, reading which parameters
    /// it takes over from `ownership` and recording there the locals it
    /// borrows. Returns true if any operation was inserted.
    pub(super) fn insert(&self, body: &mut Body, ownership: &mut OwnershipFacts) -> bool {
        // Step 0: a lent argument held only through a class field gets a local
        // of its own across a call that may release it, retained below like
        // any other copy of a counted field.
        let held = super::lent_arguments::hold_lent_field_arguments(body, ownership);

        // Step 1: Identify which variables actually need tracking (managed locals).
        //
        // NOTE: We do NOT exit early when managed_locals is empty. Even when there
        // are no non-parameter managed locals, a closure aggregate may capture a
        // managed parameter (e.g. `fn make_counter(items [int]) fn() int`). In that
        // case `handle_aggregate` must still IncRef the parameter; it uses
        // `place_is_counted` rather than `managed_locals`, so it works correctly.
        // Exiting early when managed_locals is empty would skip that IncRef, causing
        // the parameter's RC to remain at 1 while the closure holds a dangling ref.
        //
        // Per-capture DecRef at StorageDead is intentionally OMITTED: captured
        // managed values are released by the runtime destructor (`miri.$dtor.{lambda}`)
        // stored in the closure struct.  This works for both local and cross-function
        // closures and eliminates the double-free that would occur if both the
        // destructor and Perceus decremented the same capture on drop.
        let mut borrowed = BorrowedLocals::of(body, ownership);
        let mut managed_locals = self.identify_managed_locals(body, ownership, &borrowed);
        let borrows = super::local_borrows::infer_local_borrows(
            body,
            ownership,
            &PerceusContext::of(body, &borrowed),
            &managed_locals,
        );
        for local in borrows.keys() {
            managed_locals.remove(local);
            borrowed.reads.insert(*local);
        }
        ownership.borrowed_locals = borrows;
        let mut changed = self.retain_written_captures(body, &managed_locals) || held;

        // Step 2: Iterate through every block of code and inject RC instructions.

        // Split the borrow: we need mutable access to basic_blocks, but only
        // immutable access to the rest of the metadata.
        let Body {
            ref mut basic_blocks,
            ref local_decls,
            ref rc_classifier,
            ref generics,
            ref captures,
            ..
        } = *body;

        let ctx = PerceusContext {
            local_decls,
            rc_classifier,
            instance: generics.instance.as_ref(),
            type_params: &generics.type_params,
            open_params: &generics.open_params,
            closure_capture_types: &captures.closure_capture_types,
            borrowed: &borrowed,
        };

        for block_data in basic_blocks.iter_mut() {
            if self.process_block(&ctx, block_data, &managed_locals) {
                changed = true;
            }
        }

        changed
    }

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
        ownership: &OwnershipFacts,
        borrowed: &BorrowedLocals,
    ) -> std::collections::HashSet<crate::mir::Local> {
        (1..body.local_decls.len())
            .map(crate::mir::Local)
            .filter(|&local| {
                !borrowed.contains(local)
                    && !ownership.is_owned_param(local)
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
            .captures
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
        let is_managed = |place: &Place| place_is_counted(place, ctx);
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
        // A value held as its bytes read from a counted one — an optional
        // read out of the placeholder word the absent value is spelled at —
        // copies what it reads and holds no reference to it.
        if place_declared_type(dest, ctx).is_some_and(|declared| ctx.holds_value(&declared.kind)) {
            return false;
        }

        // Direct managed place?
        if place_is_counted(source, ctx) {
            return true;
        }

        // A value held as its bytes — in a class field, a struct, an element —
        // is copied, never retained.
        if place_declared_type(source, ctx).is_some_and(|declared| ctx.holds_value(&declared.kind))
        {
            return false;
        }

        // Fallback for a field read whose own type does not settle it, stored
        // somewhere that keeps a reference. What keeps it is the slot written:
        // an element or field of a counted holder is a scalar when its type
        // says so, however counted the holder is. A slot whose type cannot be
        // read is taken to keep one.
        source
            .projection
            .iter()
            .any(|e| matches!(e, PlaceElem::Field(_)))
            && managed_locals.contains(&dest.local)
            && place_declared_type(dest, ctx).is_none_or(|slot| ctx.is_counted(&slot.kind))
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
        for place in operands.iter().filter_map(Operand::place) {
            if place_is_counted(place, ctx) {
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
            let aliases_managed_source = place_is_counted(place, ctx)
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
        lhs.local.0 != 0 && !overwrites_borrowed_value && place_is_counted(lhs, ctx)
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
/// - Custom struct/class types — `Field(i)` is the field's type at the type
///   arguments of the instance it is projected from (see
///   [`crate::mir::instantiation::field_type`])
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
fn place_is_counted(place: &Place, ctx: &PerceusContext) -> bool {
    place_declared_type(place, ctx).is_some_and(|declared| ctx.is_counted(&declared.kind))
}

/// The declared type of what `place` reaches; see [`PlaceTypes::declared`].
pub(super) fn place_declared_type(place: &Place, ctx: &PerceusContext) -> Option<Type> {
    PlaceTypes {
        local_decls: ctx.local_decls,
        instance: ctx.instance,
        closure_capture_types: ctx.closure_capture_types,
        definitions: ctx.rc_classifier.definitions(),
        type_params: ctx.type_params,
        open_params: ctx.open_params,
        strict: true,
    }
    .declared(place)
}
