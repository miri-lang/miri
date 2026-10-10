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
//! reached through one of its fields is. A struct of plain scalars is copied
//! every time it is bound (`value_copy`), so no second name holds it — unless a
//! collection holds it, since cloning a collection copies its elements'
//! pointers rather than the elements.

use crate::ast::expression::{Expression, ExpressionKind};
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

/// One step of the path a write reaches through, read off the expression
/// before any code for it is emitted.
enum Step<'e> {
    /// The binding the path starts at.
    Root(Local, Type),
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
            | Step::Field(_, ty)
            | Step::Element(_, ty)
            | Step::Entry(_, _, ty) => ty,
        }
    }
}

/// How a value on a write's path is made its own before the write.
enum Unsharing {
    /// A struct holding a managed field, by name and with its fields' types
    /// as declared: rebuilt from its fields.
    Struct(String, Vec<Type>),
    /// A built-in collection: cloned by the runtime.
    Collection(BuiltinCollectionKind),
}

/// Lowers `obj`, the value a write goes into, as a place every value on whose
/// path is the program's own.
///
/// Returns `None` when `obj` is not a path of bindings, fields, collection
/// elements and map entries, leaving the caller to lower it as before. Nothing
/// is emitted in that case: the path is read in full before any of it is
/// lowered, so an index or key expression is never evaluated twice.
pub(crate) fn lower_written_place(
    ctx: &mut LoweringContext,
    obj: &Expression,
) -> Result<Option<Place>, LoweringError> {
    // Reference counts exist on the host only; a GPU body writes in place.
    if ctx.body.is_gpu() {
        return Ok(None);
    }
    let mut steps = Vec::new();
    if !collect_steps(ctx, obj, &mut steps) {
        return Ok(None);
    }
    let Some(Step::Root(root, _)) = steps.first() else {
        return Ok(None);
    };
    let mut place = Place::new(*root);
    let mut in_collection = false;
    for step in &steps {
        match step {
            Step::Root(..) => {}
            Step::Field(idx, _) => place.projection.push(PlaceElem::Field(*idx)),
            Step::Element(index, _) => {
                let index_local = lower_index_local(ctx, index)?;
                place.projection.push(PlaceElem::Index(index_local));
                in_collection = true;
            }
            Step::Entry(key, map_ty, value_ty) => {
                place = lower_entry_place(ctx, &place, map_ty, key, value_ty, obj)?;
                in_collection = true;
                continue;
            }
        }
        if let Some(unsharing) = unsharing(ctx, step.ty(), in_collection) {
            emit_unshare(ctx, &place, step.ty(), unsharing, obj.span)?;
        }
    }
    Ok(Some(place))
}

/// Reads the path `expr` names into `steps`, outermost first. False when any
/// part of it is something a write cannot be routed through here: anything but
/// a binding, a field, or an element or entry of a collection.
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
    false
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
    if is_parameter && unsharing(ctx, &decl.ty, false).is_some() {
        return None;
    }
    Some(Step::Root(local, decl.ty.clone()))
}

/// The step for field `prop` of the value the last step reached.
fn field_step<'e>(
    ctx: &LoweringContext,
    steps: &[Step<'e>],
    prop: &Expression,
    expr: &Expression,
) -> Option<Step<'e>> {
    let TypeKind::Custom(type_name, _) = &steps.last()?.ty().kind else {
        return None;
    };
    let idx = super::assignment_expr::resolve_member_field_index(
        type_name,
        prop,
        ctx.type_checker.type_definitions(),
    )?;
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
) -> Result<Place, LoweringError> {
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
    let Some(unsharing) = unsharing(ctx, value_ty, true) else {
        return Ok(entry);
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
    Ok(entry)
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
/// A struct of plain scalars is copied whenever a binding, a field or a
/// closure takes it, so outside a collection no second holder exists. A
/// collection's clone copies its elements' pointers rather than the elements,
/// so one held in a collection (`in_collection`) may be shared all the same.
fn unsharing(ctx: &LoweringContext, ty: &Type, in_collection: bool) -> Option<Unsharing> {
    if let Some(kind) = ty.kind.as_builtin_collection() {
        return Some(Unsharing::Collection(kind));
    }
    let TypeKind::Custom(name, _) = &ty.kind else {
        return None;
    };
    let Some(TypeDefinition::Struct(definition)) =
        ctx.type_checker.type_definitions().get(name.as_str())
    else {
        return None;
    };
    let declared = definition
        .fields
        .iter()
        .map(|(_, field_ty, _)| field_ty.clone())
        .collect();
    (in_collection || !ctx.is_type_auto_copy(ty)).then(|| Unsharing::Struct(name.clone(), declared))
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
/// TODO: the question is an out-of-line runtime call on every such write. Code
/// generation could read the count inline — the word before the payload,
/// compared with 1 — leaving the call only for heap-guard runs.
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
        Unsharing::Struct(name, declared) => {
            emit_struct_copy(ctx, place, ty, (&name, &declared), copy, span)
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
            Ok(())
        }
    }
}

/// Builds into `copy` a struct with the fields of the one at `place`.
///
/// Each managed field is retained by the aggregate, so the copy and the
/// original each own what they hold. A field holding a struct of plain scalars
/// is rebuilt too, because neither the original nor the copy retains it and
/// sharing it would let a write through one reach the other.
fn emit_struct_copy(
    ctx: &mut LoweringContext,
    place: &Place,
    ty: &Type,
    (name, declared): (&str, &[Type]),
    copy: Local,
    span: Span,
) -> Result<(), LoweringError> {
    let mut operands = Vec::with_capacity(declared.len());
    for (idx, declared_ty) in declared.iter().enumerate() {
        let field_ty = crate::mir::lowering::field_type_in_instance(
            &ctx.body.class_type_params,
            name,
            Some(ty),
            declared_ty,
        );
        let mut field_place = place.clone();
        field_place.projection.push(PlaceElem::Field(idx));
        operands.push(field_copy_operand(ctx, field_place, &field_ty, span)?);
    }
    ctx.push_statement(Statement {
        kind: StatementKind::Assign(
            Place::new(copy),
            Rvalue::Aggregate(AggregateKind::Struct(ty.clone()), operands),
        ),
        span,
    });
    Ok(())
}

fn field_copy_operand(
    ctx: &mut LoweringContext,
    field_place: Place,
    field_ty: &Type,
    span: Span,
) -> Result<Operand, LoweringError> {
    let rebuilt = super::value_copy::copy_value_aggregate(ctx, &field_place, field_ty, None, span)?;
    Ok(rebuilt.unwrap_or(Operand::Copy(field_place)))
}

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
            out_args: Vec::new(),
            arg_handles: Vec::new(),
            destination: Place::new(destination),
            target: Some(target),
        },
        span,
    ));
}
