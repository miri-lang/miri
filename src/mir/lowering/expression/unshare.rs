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
//! A parameter's value is its caller's, and the caller may hold it under more
//! than one name, so a write never starts at a parameter: a body that writes
//! one, or through a path starting at one, binds a copy of it on entry
//! (`lowering::written_parameters`) and every write goes into the copy. Only a
//! method's receiver, which the caller made the body's own before calling a
//! writer, and an `out` parameter, which is the caller's binding itself, are
//! written in place; reaching any other parameter here is a broken invariant,
//! reported rather than lowered into a write the caller would see.
//!
//! A path may also start at a value no binding holds — a call's result, a
//! conditional's. That value is held in a temp owning one reference for the
//! duration of the write, made its own like any binding's value, and released
//! once the write is done: a write into it is not seen by anyone else, because
//! a collection is a value.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::lowering::context::LoweringContext;
use crate::mir::lowering::expression::lower_expression;
use crate::mir::place::PlaceElem;
use crate::mir::terminator::Discriminant;
use crate::mir::type_facts::unsharing::{is_copied_on_write, unsharing, Unsharing};
use crate::mir::{
    AggregateKind, BasicBlock, Local, Operand, Place, Rvalue, Statement, StatementKind, Terminator,
    TerminatorKind,
};
use crate::runtime_fns::rt;

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
/// A map holds a value held as its bytes inline, so looking one up copies it
/// out: a member written into that copy reaches the map only once the copy is
/// stored back under the same key.
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

/// Lowers `obj`, the value a write goes into, as a place every value on whose
/// path is the program's own ([`lower_written_path`], then
/// [`LoweredPath::make_own`]), reporting the entry the place has to be stored
/// back into when it is a copy of a map's inline value.
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
/// collection is reached through one and no write-back arises.
pub(crate) fn lower_written_entry_place(
    ctx: &mut LoweringContext,
    obj: &Expression,
) -> Result<Option<WrittenPlace>, LoweringError> {
    let Some(path) = lower_written_path(ctx, obj)? else {
        return Ok(None);
    };
    path.make_own(ctx, obj).map(Some)
}

/// One step of a written path once the code computing it has run: its index
/// local or its key lowered, nothing yet checked.
enum LoweredStep {
    /// The value the path starts at.
    Root(Type),
    Field(usize, Type),
    Element(Local, Type),
    /// The entry of a map under a key already lowered.
    Entry(EntryKey, Type),
}

/// A written path whose every index and key has been computed, and whose
/// values are not yet made the program's own.
///
/// A write lowers its path first, then every other operand it needs, and
/// makes the path its own ([`LoweredPath::make_own`]) only after that, right
/// before the write: an operand computed between the check and the write
/// could hand another name the value the check found unshared, and the write
/// would then land in a value that name also holds.
#[must_use]
pub(crate) struct LoweredPath {
    root: Place,
    steps: Vec<LoweredStep>,
    temporary: Option<TemporaryRoot>,
}

impl LoweredPath {
    /// The temp the path starts at when no binding holds its value, for the
    /// caller to release once the write is done.
    pub(crate) fn take_temporary(&mut self) -> Option<TemporaryRoot> {
        self.temporary.take()
    }

    /// The place the path names now, for a read: nothing on it is checked or
    /// copied, and an entry on it is looked up as it stands.
    pub(crate) fn read_place(&self, ctx: &mut LoweringContext, obj: &Expression) -> Place {
        use super::assignment_expr::emit_map_get_checked_call;
        let mut place = self.root.clone();
        for step in &self.steps {
            match step {
                LoweredStep::Root(_) => {}
                LoweredStep::Field(idx, _) => place.projection.push(PlaceElem::Field(*idx)),
                LoweredStep::Element(index, _) => place.projection.push(PlaceElem::Index(*index)),
                LoweredStep::Entry(key, value_ty) => {
                    let map_op = Operand::Copy(place);
                    let key_op = Operand::Copy(key.place.clone());
                    let entry = emit_map_get_checked_call(ctx, map_op, key_op, value_ty, obj);
                    place = crate::mir::lowering::helpers::ensure_place(ctx, entry, obj.span);
                }
            }
        }
        place
    }

    /// Makes every value on the path the program's own, outermost first, and
    /// answers the place the write goes into.
    pub(crate) fn make_own(
        self,
        ctx: &mut LoweringContext,
        obj: &Expression,
    ) -> Result<WrittenPlace, LoweringError> {
        let mut place = self.root;
        let mut write_back = None;
        for step in self.steps {
            let ty = match step {
                LoweredStep::Root(ty) => ty,
                LoweredStep::Field(idx, ty) => {
                    place.projection.push(PlaceElem::Field(idx));
                    ty
                }
                LoweredStep::Element(index, ty) => {
                    place.projection.push(PlaceElem::Index(index));
                    ty
                }
                LoweredStep::Entry(key, value_ty) => {
                    let entry = own_entry_place(ctx, &place, key, &value_ty, obj)?;
                    place = entry.place;
                    write_back = entry.write_back;
                    continue;
                }
            };
            if let Some(unsharing) = unsharing(&ctx.body, &ty) {
                emit_unshare(ctx, &place, &ty, unsharing, obj.span)?;
            }
        }
        Ok(WrittenPlace {
            place,
            write_back,
            temporary: self.temporary,
        })
    }
}

/// Lowers the code `obj`, the value a write goes into, computes before the
/// write — the value its path starts at when no binding holds it, every index
/// and every key — and checks nothing; `None` when `obj` is not a path a write
/// is routed through here.
pub(crate) fn lower_written_path(
    ctx: &mut LoweringContext,
    obj: &Expression,
) -> Result<Option<LoweredPath>, LoweringError> {
    // Reference counts exist on the host only; a GPU body writes in place.
    if ctx.body.is_gpu() {
        return Ok(None);
    }
    let mut steps = Vec::new();
    if !collect_steps(ctx, obj, &mut steps)? {
        return Ok(None);
    }
    let Some((root, temporary)) = lower_path_root(ctx, &steps)? else {
        return Ok(None);
    };
    let mut lowered = Vec::with_capacity(steps.len());
    for step in steps {
        lowered.push(match step {
            Step::Root(_, ty) | Step::Temporary(_, ty) => LoweredStep::Root(ty),
            Step::Field(idx, ty) => LoweredStep::Field(idx, ty),
            Step::Element(index, ty) => LoweredStep::Element(lower_index_local(ctx, index)?, ty),
            Step::Entry(key, map_ty, value_ty) => lower_entry_key(ctx, key, map_ty, value_ty)?,
        });
    }
    Ok(Some(LoweredPath {
        root,
        steps: lowered,
        temporary,
    }))
}

/// The key a written path looks a map's entry up under, lowered into a
/// place of its own.
fn lower_entry_key(
    ctx: &mut LoweringContext,
    key: &Expression,
    map_ty: Type,
    value_ty: Type,
) -> Result<LoweredStep, LoweringError> {
    let key_watermark = ctx.body.local_decls.len();
    let (key_op, key_ty) = crate::mir::lowering::dispatch::lower_stored_value(
        ctx,
        key,
        &map_ty,
        crate::mir::lowering::dispatch::ELEMENT_SLOT,
    )?;
    let place = crate::mir::lowering::helpers::ensure_place(ctx, key_op, key.span);
    let key = EntryKey {
        place,
        ty: key_ty,
        watermark: key_watermark,
        span: key.span,
    };
    Ok(LoweredStep::Entry(key, value_ty))
}

/// The key of a map entry on a written path, already lowered.
struct EntryKey {
    place: Place,
    ty: Type,
    watermark: usize,
    span: Span,
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

/// One field or element step of a written path, read off the expression
/// that takes it.
pub(crate) enum PathLink<'e> {
    /// `object.prop`, the whole expression beside it.
    Member(&'e Expression, &'e Expression),
    /// `collection[index]`, the whole expression beside them.
    Index(&'e Expression, &'e Expression, &'e Expression),
}

/// The expression a written path starts at, and the field and element steps
/// taken from it, innermost first. The one reading of a path's shape that
/// both lowering a write ([`collect_steps`]) and finding the parameters a body
/// writes (`lowering::written_parameters`) go through.
pub(crate) fn path_links(expr: &Expression) -> (&Expression, Vec<PathLink<'_>>) {
    let mut links = Vec::new();
    let mut root = expr;
    loop {
        if let ExpressionKind::Member(inner, prop) = &root.node {
            links.push(PathLink::Member(prop, root));
            root = inner;
        } else if let ExpressionKind::Index(collection, index) = &root.node {
            links.push(PathLink::Index(collection, index, root));
            root = collection;
        } else {
            break;
        }
    }
    links.reverse();
    (root, links)
}

/// The expression a written path starts at: `expr` with every field and
/// element step taken off.
pub(crate) fn path_root(expr: &Expression) -> &Expression {
    path_links(expr).0
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
) -> Result<bool, LoweringError> {
    let (root, links) = path_links(expr);
    let Some(root) = root_of_path(ctx, root)? else {
        return Ok(false);
    };
    steps.push(root);
    for link in links {
        let step = match link {
            PathLink::Member(prop, member) => field_step(ctx, steps, prop, member),
            PathLink::Index(collection, index, element) => {
                indexed_step(ctx, collection, index, element)
            }
        };
        let Some(step) = step else {
            return Ok(false);
        };
        steps.push(step);
    }
    Ok(true)
}

/// The step a written path starting at `root` starts with: the binding it
/// names, or the value no binding holds; `None` for `super`.
fn root_of_path<'e>(
    ctx: &LoweringContext,
    root: &'e Expression,
) -> Result<Option<Step<'e>>, LoweringError> {
    if let ExpressionKind::Identifier(name, _) = &root.node {
        return root_step(ctx, name, root.span);
    }
    if matches!(root.node, ExpressionKind::Super) {
        return Ok(None);
    }
    Ok(ctx
        .recorded_type(root.id)
        .map(|ty| Step::Temporary(root, ty)))
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
/// A GPU-resident binding, whose host block mirrors a device buffer, is never
/// replaced by a copy here, and neither is a method's receiver, which its
/// caller made the body's own. An `out` parameter is the caller's binding
/// itself, so its count is the caller's own and the path is unshared from it
/// like from any binding. Any other parameter holding a value a write copies
/// is a value the caller still holds: the body writes it only through the
/// copy it binds on entry, so reaching one here is a write that entry missed.
fn root_step<'e>(
    ctx: &LoweringContext,
    name: &str,
    span: Span,
) -> Result<Option<Step<'e>>, LoweringError> {
    let Some(&local) = ctx.variable_map.get(name) else {
        return Ok(None);
    };
    let decl = &ctx.body.local_decls[local.0];
    if decl.device_handle.is_some() {
        return Ok(None);
    }
    let is_parameter = (1..=ctx.body.arg_count).contains(&local.0);
    if is_parameter && is_copied_on_write(&ctx.body, &decl.ty) {
        if ctx.body.receiver == Some(local) {
            return Ok(None);
        }
        if ctx.body.is_lent_param(local) {
            return Err(lent_parameter_write(name, span));
        }
    }
    Ok(Some(Step::Root(local, decl.ty.clone())))
}

/// The internal error for a write reaching the parameter `name`, whose value
/// its caller still holds, rather than the copy the body binds on entry.
pub(crate) fn lent_parameter_write(name: &str, span: Span) -> LoweringError {
    LoweringError::internal(
        DiagnosticCode::MirValidationFailed,
        format!(
            "a write reached the parameter `{name}` itself, whose value its caller still \
             holds, instead of the copy of it the body binds on entry"
        ),
        span,
    )
}

/// The step for field `prop` of the value the last step reached: a field of a
/// struct or class by its name, or an element of a tuple by its position.
fn field_step<'e>(
    ctx: &LoweringContext,
    steps: &[Step<'e>],
    prop: &Expression,
    expr: &Expression,
) -> Option<Step<'e>> {
    let idx = super::assignment_expr::field_index(ctx, steps.last()?.ty(), prop)?;
    Some(Step::Field(idx, ctx.recorded_type(expr.id)?))
}

/// Lowers the entry the map at `map` holds under `key` as a place whose value
/// only the map holds.
///
/// A map reaches its entries only through the runtime, so there is no slot to
/// write a copy into: the lookup hands back a borrow of the entry, and an entry
/// anything else also holds is copied and the copy stored back under the same
/// key — the map releasing the original — before the entry is looked up again.
fn own_entry_place(
    ctx: &mut LoweringContext,
    map: &Place,
    key: EntryKey,
    value_ty: &Type,
    obj: &Expression,
) -> Result<WrittenPlace, LoweringError> {
    use super::assignment_expr::emit_map_get_checked_call;
    let EntryKey {
        place: key_place,
        ty: key_ty,
        watermark: key_watermark,
        span: key_span,
    } = key;
    let lookup = |ctx: &mut LoweringContext| {
        let map_op = Operand::Copy(map.clone());
        let key_op = Operand::Copy(key_place.clone());
        let entry = emit_map_get_checked_call(ctx, map_op, key_op, value_ty, obj);
        crate::mir::lowering::helpers::ensure_place(ctx, entry, obj.span)
    };
    let entry = lookup(ctx);
    if ctx.body.value_layout(&value_ty.kind).is_some() {
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
    let Some(unsharing) = unsharing(&ctx.body, value_ty) else {
        ctx.emit_temp_drop(key_place.local, key_watermark, key_span);
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
        key_span,
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
    ctx.emit_temp_drop(key_place.local, key_watermark, key_span);
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
/// Note: When a collection parameter is borrowed rather than taken over, entry binding
/// retains a reference that increases reference counts, causing subsequent mutation paths
/// to copy the collection before modifying it.
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
