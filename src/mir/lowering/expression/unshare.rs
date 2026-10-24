// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Value semantics for a write that reaches through a path.
//!
//! A struct and a `List`, `Map`, `Set` or `Array` are values: `var c = b` gives
//! `c` a value of its own, and writing `c.count` must leave `b` alone. Each is
//! still stored as a pointer to a reference-counted block, so binding a second
//! name, passing one to a function or capturing it in a closure shares the
//! block and retains it. Sharing stays invisible until something writes, so a
//! write first makes every value on its path its own: each block someone else
//! also holds is replaced by a copy, outermost first, and the write then goes
//! into the copy.
//!
//! A class instance is a reference by design and is never copied, but a value
//! reached through one of its fields is. A struct or tuple held as its bytes is
//! copied every time it is bound, wherever it is held — a frame, a field, a
//! collection — so no second name ever holds it and it is never unshared.
//!
//! A path may also start at a value no binding holds — a call's result, a
//! conditional's. That value is held in a temp owning one reference for the
//! duration of the write, made its own like any binding's value, and released
//! once the write is done: a write into it is not seen by anyone else, because
//! a collection is a value.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::Literal;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::place::PlaceElem;
use crate::mir::terminator::Discriminant;
use crate::mir::{
    AggregateKind, BasicBlock, Local, Operand, Place, Rvalue, Statement, StatementKind, Terminator,
    TerminatorKind,
};
use crate::runtime_fns::rt;
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::element_layout::element_layout;

/// One step of the path a write reaches through, read off the expression
/// before any code for it is emitted.
enum Step<'e> {
    /// The binding the path starts at.
    Root(Local, Type),
    /// The value the path starts at when no binding holds it: anything but a
    /// binding, a field, an element or `super`.
    Temporary(&'e Expression, Type),
    /// A field of the value before it, by its index in that value's layout.
    Field(usize, Type),
    /// An element of the `List` or `Array` before it.
    Element(&'e Expression, Type),
    /// The entry of the `Map` before it under a key, with the map's type.
    Entry(&'e Expression, Type, Type),
}

impl Step<'_> {
    fn ty(&self) -> &Type {
        match self {
            Step::Root(_, ty)
            | Step::Temporary(_, ty)
            | Step::Field(_, ty)
            | Step::Element(_, ty)
            | Step::Entry(_, _, ty) => ty,
        }
    }
}

/// How a value on a write's path is made its own before the write.
enum Unsharing {
    /// A struct held as a reference-counted allocation, with this many
    /// fields: rebuilt from its fields.
    Struct(usize),
    /// A tuple held as a reference-counted allocation, with this many
    /// elements: rebuilt from its elements.
    Tuple(usize),
    /// A built-in collection: cloned by the runtime.
    Collection(BuiltinCollectionKind),
}

/// The temp a written path starts at when no binding holds the value, with
/// the mark taken before that value was lowered.
///
/// It holds one reference of its own for the duration of the write and gives
/// it back once the write is done; a temp an enclosing scope owns, or one that
/// borrows what it holds, is left to its owner.
#[must_use]
pub(crate) struct TemporaryRoot {
    local: Local,
    watermark: usize,
}

impl TemporaryRoot {
    /// The temp `local`, lowered after `watermark`, that a receiver lowered
    /// outside a written path landed in.
    pub(crate) fn new(local: Local, watermark: usize) -> Self {
        Self { local, watermark }
    }

    pub(crate) fn local(&self) -> Local {
        self.local
    }

    /// Gives back the reference the temp `temporary` holds, if there is one,
    /// the write into it being done.
    pub(crate) fn release(temporary: Option<Self>, ctx: &mut LoweringContext, span: Span) {
        if let Some(temporary) = temporary {
            ctx.emit_temp_drop(temporary.local, temporary.watermark, span);
        }
    }
}

/// A map entry a write lands in that the lookup handed back as a copy.
///
/// A map holds a vector inline, as its components' bytes, so looking one up
/// copies it out: a component written into that copy reaches the map only
/// once the copy is stored back under the same key.
pub(crate) struct EntryWriteBack {
    map: Place,
    key: Place,
    key_ty: Type,
    key_watermark: usize,
    value: Place,
}

/// The place a write goes into, the entry it has to be stored back into
/// afterwards when the place is a copy of a map's inline value, and the temp
/// the path starts at when no binding holds its value.
#[must_use]
pub(crate) struct WrittenPlace {
    pub place: Place,
    write_back: Option<EntryWriteBack>,
    temporary: Option<TemporaryRoot>,
}

impl WrittenPlace {
    /// Completes the write into `obj` once it is done: stores a copied-out map
    /// value back under its key, then releases the temp the path starts at.
    pub(crate) fn finish(self, ctx: &mut LoweringContext, obj: &Expression) {
        if let Some(write_back) = self.write_back {
            store_entry_back(ctx, write_back, obj);
        }
        TemporaryRoot::release(self.temporary, ctx, obj.span);
    }
}

/// Lowers `obj`, the value a write goes into, as a place every value on whose
/// path is the program's own.
///
/// Returns `None` when `obj` is not a path of fields, collection elements and
/// map entries starting at a binding a write may replace or at a value no
/// binding holds, leaving the caller to lower it as before. Nothing is emitted
/// in that case: the path is read in full before any of it is lowered, so an
/// index or key expression is never evaluated twice.
///
/// For a collection receiver — what an indexed store or a mutating method
/// writes into — the place is the collection itself, never a copy of a map's
/// inline value: a value held as its bytes holds nothing counted, so no
/// collection is reached through one and no write-back arises. A component
/// write calls [`lower_written_entry_place`] instead. The temp the path starts
/// at, when no binding holds its value, is returned beside the place for the
/// caller to release once the write is done.
pub(crate) fn lower_written_place(
    ctx: &mut LoweringContext,
    obj: &Expression,
) -> Result<Option<(Place, Option<TemporaryRoot>)>, LoweringError> {
    Ok(lower_written_entry_place(ctx, obj)?.map(|written| (written.place, written.temporary)))
}

/// Stores a copied-out map value back under its key once the write into it is
/// done, the map taking the key, then releases the copy.
fn store_entry_back(ctx: &mut LoweringContext, write_back: EntryWriteBack, obj: &Expression) {
    let (donated_key, _) = crate::mir::lowering::dispatch::donate_operand_to_container(
        ctx,
        Operand::Copy(write_back.key.clone()),
        write_back.key_ty,
        obj.span,
    );
    super::assignment_expr::emit_map_set_call(
        ctx,
        Operand::Copy(write_back.map),
        donated_key,
        Operand::Copy(write_back.value.clone()),
        obj,
    );
    ctx.emit_temp_drop(write_back.key.local, write_back.key_watermark, obj.span);
    let copy = write_back.value.local;
    ctx.emit_temp_drop(copy, copy.0, obj.span);
}

/// [`lower_written_place`], reporting the entry the place has to be stored
/// back into when it is a copy of a map's inline value.
pub(crate) fn lower_written_entry_place(
    ctx: &mut LoweringContext,
    obj: &Expression,
) -> Result<Option<WrittenPlace>, LoweringError> {
    // Reference counts exist on the host only; a GPU body writes in place.
    if ctx.body.is_gpu() {
        return Ok(None);
    }
    let mut steps = Vec::new();
    if !collect_steps(ctx, obj, &mut steps) {
        return Ok(None);
    }
    let Some((mut place, temporary)) = lower_path_root(ctx, &steps)? else {
        return Ok(None);
    };
    let mut write_back = None;
    for step in &steps {
        match step {
            Step::Root(..) | Step::Temporary(..) => {}
            Step::Field(idx, _) => place.projection.push(PlaceElem::Field(*idx)),
            Step::Element(index, _) => {
                let index_local = lower_index_local(ctx, index)?;
                place.projection.push(PlaceElem::Index(index_local));
            }
            Step::Entry(key, map_ty, value_ty) => {
                let entry = lower_entry_place(ctx, &place, map_ty, key, value_ty, obj)?;
                place = entry.place;
                write_back = entry.write_back;
                continue;
            }
        }
        if let Some(unsharing) = unsharing(ctx, step.ty()) {
            emit_unshare(ctx, &place, step.ty(), unsharing, obj.span)?;
        }
    }
    Ok(Some(WrittenPlace {
        place,
        write_back,
        temporary,
    }))
}

/// The place the path `steps` starts at — a binding, or a temp holding the
/// value no binding holds, returned beside it — or `None` when the path starts
/// at neither.
fn lower_path_root(
    ctx: &mut LoweringContext,
    steps: &[Step],
) -> Result<Option<(Place, Option<TemporaryRoot>)>, LoweringError> {
    Ok(match steps.first() {
        Some(Step::Root(root, _)) => Some((Place::new(*root), None)),
        Some(Step::Temporary(root, ty)) => {
            let temporary = lower_temporary_root(ctx, root, ty)?;
            Some((Place::new(temporary.local), Some(temporary)))
        }
        Some(Step::Field(..) | Step::Element(..) | Step::Entry(..)) | None => None,
    })
}

/// Lowers `root`, the value of type `ty` a written path starts at when no
/// binding holds it, into a temp holding one reference of its own.
///
/// A value lowering leaves in a fresh temp that owns it — a call's result, a
/// conditional's — is used as it is. Anything else is copied into a temp of
/// its own, and what it was read from released at once: while both held it,
/// the check before the write would find every such value shared and copy it.
fn lower_temporary_root(
    ctx: &mut LoweringContext,
    root: &Expression,
    ty: &Type,
) -> Result<TemporaryRoot, LoweringError> {
    let watermark = ctx.body.local_decls.len();
    let operand = lower_expression(ctx, root, None)?;
    let source = match &operand {
        Operand::Copy(place) | Operand::Move(place) => Some(place),
        Operand::Constant(_) | Operand::Function(_) => None,
    };
    if let Some(place) = source {
        if place.projection.is_empty() && ctx.is_expression_temp(place.local, watermark) {
            return Ok(TemporaryRoot::new(place.local, watermark));
        }
    }
    let source = source.map(|place| place.local);
    let local = ctx.push_temp(ty.clone(), root.span);
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(
            Place::new(local),
            Rvalue::Use(crate::mir::lowering::dispatch::move_to_copy(operand)),
        ),
        span: root.span,
    });
    if let Some(source) = source {
        ctx.emit_temp_drop(source, watermark, root.span);
    }
    Ok(TemporaryRoot::new(local, watermark))
}

/// Reads the path `expr` names into `steps`, outermost first. False when any
/// part of it is something a write cannot be routed through here: a binding
/// a write may not replace, a member that is not a field, an element of a
/// `Set`, or `super`. A path starting at a value no binding holds starts at a
/// [`Step::Temporary`].
fn collect_steps<'e>(
    ctx: &LoweringContext,
    expr: &'e Expression,
    steps: &mut Vec<Step<'e>>,
) -> bool {
    if let ExpressionKind::Identifier(name, _) = &expr.node {
        return root_step(ctx, name).is_some_and(|root| {
            steps.push(root);
            true
        });
    }
    if let ExpressionKind::Member(inner, prop) = &expr.node {
        return collect_steps(ctx, inner, steps)
            && field_step(ctx, steps, prop, expr).is_some_and(|step| {
                steps.push(step);
                true
            });
    }
    if let ExpressionKind::Index(collection, index) = &expr.node {
        let Some(step) = indexed_step(ctx, collection, index, expr) else {
            return false;
        };
        let reached = collect_steps(ctx, collection, steps);
        steps.push(step);
        return reached;
    }
    if matches!(expr.node, ExpressionKind::Super) {
        return false;
    }
    ctx.recorded_type(expr.id).is_some_and(|ty| {
        steps.push(Step::Temporary(expr, ty));
        true
    })
}

/// The step for `collection[index]`: an element slot of a `List` or `Array`,
/// or an entry of a `Map`. A `Set` has no slot an index reaches.
fn indexed_step<'e>(
    ctx: &LoweringContext,
    collection: &Expression,
    index: &'e Expression,
    expr: &Expression,
) -> Option<Step<'e>> {
    let collection_ty = ctx.recorded_type(collection.id)?;
    let ty = ctx.recorded_type(expr.id)?;
    match collection_ty.kind.as_builtin_collection()? {
        BuiltinCollectionKind::List | BuiltinCollectionKind::Array => {
            Some(Step::Element(index, ty))
        }
        BuiltinCollectionKind::Map => Some(Step::Entry(index, collection_ty, ty)),
        BuiltinCollectionKind::Set => None,
    }
}

/// The step for the binding `name`, when a write may replace its value.
///
/// A parameter's value belongs to the caller, so a binding the body only
/// borrows is never replaced by a copy here; neither is a GPU-resident binding,
/// whose host block mirrors a device buffer.
fn root_step<'e>(ctx: &LoweringContext, name: &str) -> Option<Step<'e>> {
    let local = *ctx.variable_map.get(name)?;
    let decl = &ctx.body.local_decls[local.0];
    if decl.device_handle.is_some() {
        return None;
    }
    let is_parameter = (1..=ctx.body.arg_count).contains(&local.0);
    if is_parameter && unsharing(ctx, &decl.ty).is_some() {
        return None;
    }
    Some(Step::Root(local, decl.ty.clone()))
}

/// The step for field `prop` of the value the last step reached: a field of a
/// struct or class by its name, or an element of a tuple by its position.
fn field_step<'e>(
    ctx: &LoweringContext,
    steps: &[Step<'e>],
    prop: &Expression,
    expr: &Expression,
) -> Option<Step<'e>> {
    let base = &steps.last()?.ty().kind;
    let idx = if let TypeKind::Custom(type_name, _) = base {
        super::assignment_expr::resolve_member_field_index(
            type_name,
            prop,
            ctx.type_checker.type_definitions(),
        )?
    } else if let (TypeKind::Tuple(_), ExpressionKind::Literal(Literal::Integer(position))) =
        (base, &prop.node)
    {
        super::member_expr::extract_integer_index(position)
    } else {
        return None;
    };
    Some(Step::Field(idx, ctx.recorded_type(expr.id)?))
}

/// Lowers the entry the map at `map` holds under `key` as a place whose value
/// only the map holds.
///
/// A map reaches its entries only through the runtime, so there is no slot to
/// write a copy into: the lookup hands back a borrow of the entry, and an entry
/// anything else also holds is copied and the copy stored back under the same
/// key — the map releasing the original — before the entry is looked up again.
fn lower_entry_place(
    ctx: &mut LoweringContext,
    map: &Place,
    map_ty: &Type,
    key: &Expression,
    value_ty: &Type,
    obj: &Expression,
) -> Result<WrittenPlace, LoweringError> {
    use super::assignment_expr::emit_map_get_checked_call;
    let key_watermark = ctx.body.local_decls.len();
    let (key_op, key_ty) = crate::mir::lowering::dispatch::lower_stored_value(
        ctx,
        key,
        map_ty,
        crate::mir::lowering::dispatch::ELEMENT_SLOT,
    )?;
    let key_place = crate::mir::lowering::helpers::ensure_place(ctx, key_op, key.span);
    let lookup = |ctx: &mut LoweringContext| {
        let map_op = Operand::Copy(map.clone());
        let key_op = Operand::Copy(key_place.clone());
        let entry = emit_map_get_checked_call(ctx, map_op, key_op, value_ty, obj);
        crate::mir::lowering::helpers::ensure_place(ctx, entry, obj.span)
    };
    let entry = lookup(ctx);
    let is_held_inline = element_layout(&value_ty.kind, ctx.body.layout_scope()).is_address
        || ctx.body.value_layout(&value_ty.kind).is_some();
    if is_held_inline {
        return Ok(WrittenPlace {
            place: entry.clone(),
            write_back: Some(EntryWriteBack {
                map: map.clone(),
                key: key_place,
                key_ty,
                key_watermark,
                value: entry,
            }),
            temporary: None,
        });
    }
    let Some(unsharing) = unsharing(ctx, value_ty) else {
        ctx.emit_temp_drop(key_place.local, key_watermark, key.span);
        return Ok(WrittenPlace {
            place: entry,
            write_back: None,
            temporary: None,
        });
    };
    let join_bb = branch_when_shared(ctx, &entry, obj.span);
    let copy = ctx.push_temp(value_ty.clone(), obj.span);
    emit_copy(ctx, &entry, value_ty, unsharing, copy, obj.span)?;
    // The map takes over both the key it is handed and the copy, so neither
    // temp holding them is released on its own.
    let (donated_key, _) = crate::mir::lowering::dispatch::donate_operand_to_container(
        ctx,
        Operand::Copy(key_place.clone()),
        key_ty,
        key.span,
    );
    let copy_op = Operand::Copy(Place::new(copy));
    super::assignment_expr::emit_map_set_call(
        ctx,
        Operand::Copy(map.clone()),
        donated_key,
        copy_op,
        obj,
    );
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: join_bb },
        obj.span,
    ));
    ctx.set_current_block(join_bb);
    let entry = lookup(ctx);
    ctx.emit_temp_drop(key_place.local, key_watermark, key.span);
    Ok(WrittenPlace {
        place: entry,
        write_back: None,
        temporary: None,
    })
}

fn lower_index_local(
    ctx: &mut LoweringContext,
    index: &Expression,
) -> Result<Local, LoweringError> {
    let operand = lower_expression(ctx, index, None)?;
    let operand = crate::mir::lowering::helpers::index_at_int(ctx, operand, index);
    Ok(super::index_expr::ensure_index_local(ctx, index, operand))
}

/// How a value of type `ty` is made its own before a write, or `None` when a
/// value of that type is never shared or is shared by design.
///
/// A value held as its bytes is copied whenever a binding, a field, an element
/// or a closure takes it, so no second holder of it ever exists. Any other
/// struct is a reference-counted allocation that a copy shares, and a write
/// copies it first. A vector is never shared either: a collection holds it
/// inline, as its components' bytes, and a copy built for it would be stored
/// by value and never released.
fn unsharing(ctx: &LoweringContext, ty: &Type) -> Option<Unsharing> {
    if let Some(kind) = ty.kind.as_builtin_collection() {
        return Some(Unsharing::Collection(kind));
    }
    if crate::ast::types::vec_type_dim(&ty.kind).is_some()
        || ctx.body.value_layout(&ty.kind).is_some()
    {
        return None;
    }
    if let TypeKind::Tuple(elements) = &ty.kind {
        return Some(Unsharing::Tuple(elements.len()));
    }
    let TypeKind::Custom(name, _) = &ty.kind else {
        return None;
    };
    let Some(TypeDefinition::Struct(definition)) =
        ctx.type_checker.type_definitions().get(name.as_str())
    else {
        return None;
    };
    Some(Unsharing::Struct(definition.fields.len()))
}

/// Replaces the value at `place` with a copy of it when anything else holds it.
///
/// The copy is written back with `Reassign`, which releases the reference the
/// place held on the shared original; the original stays alive in whoever else
/// holds it, so the release never frees it.
fn emit_unshare(
    ctx: &mut LoweringContext,
    place: &Place,
    ty: &Type,
    unsharing: Unsharing,
    span: Span,
) -> Result<(), LoweringError> {
    let join_bb = branch_when_shared(ctx, place, span);
    // The copy's reference passes to `place`, so the temp holding it on the
    // way is never released on its own.
    let copy = ctx.push_temp(ty.clone(), span);
    emit_copy(ctx, place, ty, unsharing, copy, span)?;
    let write_back = Rvalue::Use(Operand::Move(Place::new(copy)));
    // An element slot releases the element it replaces as part of the store,
    // the way every element write does; any other place releases it through
    // `Reassign`.
    let kind = if matches!(place.projection.last(), Some(PlaceElem::Index(_))) {
        StatementKind::Assign(place.clone(), write_back)
    } else {
        StatementKind::Reassign(place.clone(), write_back)
    };
    ctx.push_statement(Statement { kind, span });
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Goto { target: join_bb },
        span,
    ));
    ctx.set_current_block(join_bb);
    Ok(())
}

/// Asks whether the value at `place` is shared and continues, in a new
/// current block, only when it is. Returns the block both paths join at; the
/// caller ends the shared path with a jump there.
///
/// Code generation answers the question inline from the count word, and calls
/// the runtime only while the heap is being tracked. Every write asks again;
/// `optimization::unshare_elision` later removes each check an earlier answer
/// or a freshly built value has already settled, so the shape emitted here —
/// the call, then a block doing nothing but branching on its answer — is what
/// that pass recognizes.
fn branch_when_shared(ctx: &mut LoweringContext, place: &Place, span: Span) -> BasicBlock {
    let copy_bb = ctx.new_basic_block();
    let join_bb = ctx.new_basic_block();
    let is_shared = ctx.push_temp(Type::new(TypeKind::Boolean, span), span);
    let check_bb = ctx.new_basic_block();
    emit_runtime_call(ctx, rt::RC_IS_SHARED, place, is_shared, check_bb, span);
    ctx.set_current_block(check_bb);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(is_shared)),
            targets: vec![(Discriminant::from(0), join_bb)],
            otherwise: copy_bb,
        },
        span,
    ));
    ctx.set_current_block(copy_bb);
    join_bb
}

/// Builds into `copy` a value of type `ty` equal to the one at `place` and
/// owning references of its own to everything it holds.
fn emit_copy(
    ctx: &mut LoweringContext,
    place: &Place,
    ty: &Type,
    unsharing: Unsharing,
    copy: Local,
    span: Span,
) -> Result<(), LoweringError> {
    match unsharing {
        Unsharing::Struct(field_count) => {
            let kind = AggregateKind::Struct(ty.clone());
            emit_aggregate_copy(ctx, place, kind, field_count, copy, span);
        }
        Unsharing::Tuple(element_count) => {
            let kind = AggregateKind::Tuple(ty.clone());
            emit_aggregate_copy(ctx, place, kind, element_count, copy, span);
        }
        Unsharing::Collection(kind) => {
            let written_bb = ctx.new_basic_block();
            emit_runtime_call(
                ctx,
                collection_clone_fn(kind),
                place,
                copy,
                written_bb,
                span,
            );
            ctx.set_current_block(written_bb);
        }
    }
    emit_copy_count(ctx, span);
    Ok(())
}

/// Reports the copy just made to the runtime's copy counter
/// (`MIRI_RC_STATS`); code generation emits the call only while the runtime
/// is tracking.
fn emit_copy_count(ctx: &mut LoweringContext, span: Span) {
    let counted_bb = ctx.new_basic_block();
    let unit = ctx.push_temp(Type::new(TypeKind::Void, span), span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func: Operand::runtime(rt::RC_STATS_COW_COPY, span),
            args: Vec::new(),
            arg_modes: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(unit),
            target: Some(counted_bb),
        },
        span,
    ));
    ctx.set_current_block(counted_bb);
}

/// Builds into `copy` an aggregate of `kind` — a struct or a tuple — with the
/// `field_count` fields of the one at `place`.
///
/// Each managed field is retained by the aggregate, so the copy and the
/// original each own what they hold; a value held as its bytes is copied as
/// those bytes.
fn emit_aggregate_copy(
    ctx: &mut LoweringContext,
    place: &Place,
    kind: AggregateKind,
    field_count: usize,
    copy: Local,
    span: Span,
) {
    let operands = (0..field_count)
        .map(|idx| {
            let mut field_place = place.clone();
            field_place.projection.push(PlaceElem::Field(idx));
            Operand::Copy(field_place)
        })
        .collect();
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(Place::new(copy), Rvalue::Aggregate(kind, operands)),
        span,
    });
}

/// The runtime copy a shared collection is replaced with before a write.
///
/// TODO: a collection passed to a callee that does not take the parameter
/// over (`optimization::owned_params` declines methods, closures and every
/// function it cannot prove has one body and plain callers) arrives here
/// shared even when the caller never uses it again (`xs = step(xs)`), because
/// the callee's `var x = param` takes a reference of its own, so the first
/// write copies the whole collection and then every element it writes.
fn collection_clone_fn(kind: BuiltinCollectionKind) -> &'static str {
    match kind {
        BuiltinCollectionKind::List => rt::LIST_CLONE,
        BuiltinCollectionKind::Map => rt::MAP_CLONE,
        BuiltinCollectionKind::Set => rt::SET_CLONE,
        BuiltinCollectionKind::Array => rt::ARRAY_CLONE,
    }
}

/// Calls runtime function `name` on the value at `place`, which it borrows,
/// writing the result into `destination` and continuing at `target`.
fn emit_runtime_call(
    ctx: &mut LoweringContext,
    name: &str,
    place: &Place,
    destination: Local,
    target: BasicBlock,
    span: Span,
) {
    let func = Operand::runtime(name, span);
    ctx.set_terminator(Terminator::new(
        TerminatorKind::Call {
            func,
            args: vec![Operand::Copy(place.clone())],
            arg_modes: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(destination),
            target: Some(target),
        },
        span,
    ));
}
