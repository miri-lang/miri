// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which calls can reach the header of a collection a body holds.
//!
//! A loop may read the header of a collection `c` once, before the loop, only
//! while no code it runs changes that header. A function of the program can
//! change it only by writing the collection through a binding that holds it
//! without a reference of its own: every write to a collection first copies
//! it when anything else holds it (copy-on-write at the call naming the
//! receiver, and the uniqueness check in a method writing its receiver
//! through another method), so a holder with a reference of its own writes
//! a copy and leaves `c`'s header as it was.
//!
//! That rests on two facts reference counting keeps:
//!
//! 1. A local living across blocks holds a reference of its own. Reference
//!    counting cancels a retain against a release only inside one block, and
//!    hands a reference on to a callee only for an owned argument named at
//!    the call and dying there.
//! 2. A holder outside `c`'s names has its own reference, so `c`'s
//!    collection is shared while it lives and a write through it copies or
//!    traps.
//!
//! The names of `c` are its [`HeaderReach`] aliases: every local a value of
//! `c` may flow into, whole or through a part that may hold one. A call to a
//! function of the program leaves `c`'s header unchanged when `c` holds a
//! reference of its own that nothing else can see — it is counted, not
//! borrowed, not a parameter it borrows, not captured, not gpu-resident, not
//! address-taken, and each of its definitions hands it a reference — and the
//! call names none of its aliases, nor a borrow of one that may hold such a
//! collection. A call into the runtime leaves every header unchanged when
//! [`runtime_call_leaves_headers`] says so: the entry point is one
//! `runtime_fns::leaves_collection_headers_unchanged` lists, or it copies an
//! array or a list (`runtime_fns::copies_collection`) whose elements carry
//! no `clone` method of the program's own. A call through a vtable or a
//! kernel launch is never known to.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

use crate::ast::gpu_wire::collection_element_kind;
use crate::ast::types::{Type, TypeKind};
use crate::mir::instantiation::shape::constructor_parts;
use crate::mir::place_type::PlaceTypes;
use crate::mir::type_facts::TypeDefinition;
use crate::mir::{
    BasicBlock, Body, Local, Operand, Place, Rvalue, Statement, StatementKind, Terminator,
    TerminatorKind,
};
use crate::runtime_fns::{copies_collection, leaves_collection_headers_unchanged};

/// Why a collection's local does not hold a reference of its own that only
/// its aliases can reach, so a call to a function of the program may change
/// its header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UncountedRoot {
    /// The local's value is not reference counted.
    NotCounted,
    /// The alias is a parameter the body does not take over.
    Parameter(Local),
    /// The alias borrows a value it holds no reference to.
    Borrowed(Local),
    /// The alias is a captured binding.
    Captured(Local),
    /// The alias is a gpu-resident binding.
    DeviceResident(Local),
    /// The alias's address is taken.
    AddressTaken(Local),
    /// The block gives the local a value it holds no reference to.
    UncountedDefinition(BasicBlock),
}

/// What can reach the header of one collection local of a body: the facts
/// every call in a loop over it is checked against, worked out the first
/// time a call to a function of the program needs them.
#[derive(Debug)]
pub struct HeaderReach<'a> {
    body: &'a Body,
    local: Local,
    facts: OnceCell<AliasFacts>,
}

/// The aliases of a collection local, and why it holds no reference of its
/// own, if it does not.
#[derive(Debug)]
struct AliasFacts {
    target: Option<Type>,
    aliases: HashSet<Local>,
    root: Option<UncountedRoot>,
}

impl<'a> HeaderReach<'a> {
    /// What can reach the header of the collection `local` holds in `body`.
    pub fn of(body: &'a Body, local: Local) -> Self {
        Self {
            body,
            local,
            facts: OnceCell::new(),
        }
    }

    fn facts(&self) -> &AliasFacts {
        self.facts.get_or_init(|| {
            let body = self.body;
            let target = body
                .local_decls
                .get(self.local.0)
                .map(|decl| decl.ty.clone());
            let aliases = target.as_ref().map_or_else(
                || HashSet::from([self.local]),
                |target| aliases_of(body, &PlaceTypes::of(body), self.local, target),
            );
            let root = uncounted_root(body, self.local, &aliases);
            AliasFacts {
                target,
                aliases,
                root,
            }
        })
    }

    /// Why the collection's local holds no reference of its own that only
    /// its aliases reach; `None` when it holds one.
    pub fn uncounted_root(&self) -> Option<UncountedRoot> {
        self.facts().root
    }

    /// Whether `terminator` reaches no code that could change the header of
    /// the collection.
    pub fn call_leaves_header(&self, terminator: &Terminator) -> bool {
        match &terminator.kind {
            TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => true,
            TerminatorKind::Call {
                func,
                args,
                destination,
                ..
            } => match func.called_runtime_name() {
                Some(name) => runtime_call_leaves_headers(self.body, name, args),
                None => {
                    let facts = self.facts();
                    facts.root.is_none()
                        && !facts.aliases.contains(&destination.local)
                        && (std::iter::once(func).chain(args))
                            .all(|operand| !facts.reaches_through(self.body, operand))
                }
            },
            TerminatorKind::VirtualCall { .. } | TerminatorKind::GpuLaunch { .. } => false,
        }
    }
}

/// Whether a call in `body` to the runtime entry point `name`, handed
/// `args`, leaves the header of every collection that exists before it as it
/// found it.
///
/// Besides the entry points `leaves_collection_headers_unchanged` lists,
/// that holds of a copy of an array or a list (`copies_collection`) whose
/// elements run no code of the program when copied. Such a call writes only
/// the place its result goes to, which a reader of the header checks like
/// any other write of a local.
pub fn runtime_call_leaves_headers(body: &Body, name: &str, args: &[Operand]) -> bool {
    leaves_collection_headers_unchanged(name)
        || (copies_collection(name)
            && args
                .first()
                .is_some_and(|copied| copy_runs_no_program_code(body, copied)))
}

/// Whether copying the array or list `copied` names calls no `clone` method
/// of the program: its element type is spelled out, and is a struct, an
/// enum or a type whose spelling shows every part of it — never a class, a
/// trait, a type parameter or anything else whose values may be instances
/// of a class, the only kind of element a copy runs a `clone` method for.
fn copy_runs_no_program_code(body: &Body, copied: &Operand) -> bool {
    let Some(place) = copied.place().filter(|place| place.projection.is_empty()) else {
        return false;
    };
    (body.local_decls.get(place.local.0))
        .and_then(|decl| collection_element_kind(&decl.ty.kind))
        .is_some_and(|element| !is_opaque(&element) || is_data_type(body, &element))
}

/// Whether `kind` names a struct or an enum: a copy calls the `clone` method
/// of a class element only, never of one of these.
fn is_data_type(body: &Body, kind: &TypeKind) -> bool {
    let TypeKind::Custom(name, _) = kind else {
        return false;
    };
    match body.rc_classifier.definitions().get(name) {
        Some(TypeDefinition::Struct(_) | TypeDefinition::Enum(_)) => true,
        Some(
            TypeDefinition::Class(_)
            | TypeDefinition::Trait(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_),
        )
        | None => false,
    }
}

impl AliasFacts {
    /// Whether a callee handed `operand` may reach the collection: it reads
    /// an alias, or a borrow of one whose type may hold the collection.
    fn reaches_through(&self, body: &Body, operand: &Operand) -> bool {
        let Some(root) = operand.place().map(|place| place.local) else {
            return false;
        };
        if self.aliases.contains(&root) {
            return true;
        }
        let borrows_an_alias = (body.borrowed_locals.get(&root))
            .is_some_and(|owners| owners.iter().any(|owner| self.aliases.contains(owner)));
        let declared = body.local_decls.get(root.0).map(|decl| &decl.ty);
        let may_hold = match (declared, &self.target) {
            (Some(ty), Some(target)) => may_hold_counted(body, ty, target),
            (None, _) | (_, None) => true,
        };
        borrows_an_alias && may_hold
    }
}

/// Whether a value of `ty` may be, or hold, a value of `target`'s type: a
/// type whose contents its spelling does not show — a declared type, a
/// type parameter, a function, a raw pointer, an identifier — may hold
/// anything.
pub fn may_hold_type(ty: &Type, target: &Type) -> bool {
    may_hold_within(ty, target, 0)
}

fn may_hold_within(ty: &Type, target: &Type, depth: usize) -> bool {
    const MAX_DEPTH: usize = 16;
    if depth > MAX_DEPTH || is_opaque(&ty.kind) || is_same_type(ty, target) {
        return true;
    }
    (constructor_parts(ty).1.iter()).any(|part| may_hold_within(part, target, depth + 1))
}

/// Whether the spelling of a `kind` hides what its values hold: its parts
/// are not the types it may reach.
fn is_opaque(kind: &TypeKind) -> bool {
    match kind {
        TypeKind::Generic(..) | TypeKind::Function(_) | TypeKind::RawPtr | TypeKind::Identifier => {
            true
        }
        TypeKind::Custom(..) => kind.as_builtin_collection().is_none(),
        TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::List(_)
        | TypeKind::Array(..)
        | TypeKind::Map(..)
        | TypeKind::Tuple(_)
        | TypeKind::Set(_)
        | TypeKind::Result(..)
        | TypeKind::Future(_)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_)
        | TypeKind::OneOf(_) => false,
    }
}

/// Whether `a` and `b` spell one type, constructor by constructor.
fn is_same_type(a: &Type, b: &Type) -> bool {
    let (a_name, a_parts) = constructor_parts(a);
    let (b_name, b_parts) = constructor_parts(b);
    a_name == b_name
        && a_parts.len() == b_parts.len()
        && a_parts
            .iter()
            .zip(&b_parts)
            .all(|(a, b)| is_same_type(a, b))
}

// Note: A class-typed element (counted, an opaque declared type) is assumed
// to potentially hold any collection, so loop iterations passing such elements
// to calls may conservatively invalidate stable collection headers unless
// field types and subclass hierarchies are fully resolved.
/// [`may_hold_type`], except that a value `body` holds as its bytes holds
/// nothing counted, so no collection.
fn may_hold_counted(body: &Body, ty: &Type, target: &Type) -> bool {
    body.value_layout(&ty.kind).is_none() && may_hold_type(ty, target)
}

/// Every local a value of `local` may flow into: through an assignment
/// reading it whole or through a part that may hold a `target`, into the
/// root of the place written; and through a call naming it anywhere, into
/// every place the call writes.
fn aliases_of(body: &Body, places: &PlaceTypes, local: Local, target: &Type) -> HashSet<Local> {
    let mut flows: HashMap<Local, Vec<Local>> = HashMap::new();
    for data in &body.basic_blocks {
        for statement in &data.statements {
            for (from, into) in statement_flows(body, places, statement, target) {
                flows.entry(from).or_default().push(into);
            }
        }
        if let Some(terminator) = &data.terminator {
            for (from, into) in terminator_flows(terminator) {
                flows.entry(from).or_default().push(into);
            }
        }
    }
    let mut aliases = HashSet::from([local]);
    let mut pending = vec![local];
    while let Some(from) = pending.pop() {
        for into in flows.get(&from).into_iter().flatten() {
            if aliases.insert(*into) {
                pending.push(*into);
            }
        }
    }
    aliases
}

/// The `(from, into)` pairs of locals `statement` may carry a collection
/// between.
fn statement_flows(
    body: &Body,
    places: &PlaceTypes,
    statement: &Statement,
    target: &Type,
) -> Vec<(Local, Local)> {
    let (StatementKind::Assign(written, rvalue) | StatementKind::Reassign(written, rvalue)) =
        &statement.kind
    else {
        return Vec::new();
    };
    let carries = |place: &Place| {
        place.projection.is_empty()
            || places
                .declared(place)
                .is_none_or(|ty| may_hold_counted(body, &ty, target))
    };
    carried_places(rvalue)
        .into_iter()
        .filter(|place| carries(place))
        .map(|place| (place.local, written.local))
        .collect()
}

/// The places whose value `rvalue` may hand on: every operand of a copy, a
/// cast or an aggregate, and the place a reference points at.
fn carried_places(rvalue: &Rvalue) -> Vec<&Place> {
    match rvalue {
        Rvalue::Use(operand) => operand.place().into_iter().collect(),
        Rvalue::Cast(operand, _) => operand.place().into_iter().collect(),
        Rvalue::Aggregate(_, operands) => operands.iter().filter_map(Operand::place).collect(),
        Rvalue::Ref(place) => vec![place],
        Rvalue::BinaryOp(..)
        | Rvalue::UnaryOp(..)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::AtomicOp { .. } => Vec::new(),
    }
}

/// The `(from, into)` pairs of locals `terminator` may carry a collection
/// between: from the root of everything it is handed into the root of
/// everything it writes.
fn terminator_flows(terminator: &Terminator) -> Vec<(Local, Local)> {
    let handed: Vec<&Operand> = match &terminator.kind {
        TerminatorKind::Call { func, args, .. } => std::iter::once(func).chain(args).collect(),
        TerminatorKind::VirtualCall { args, .. } => args.iter().collect(),
        TerminatorKind::GpuLaunch {
            kernel,
            launch_args,
            scalar_args,
            ..
        } => (std::iter::once(kernel).chain(launch_args.args()))
            .chain(scalar_args)
            .collect(),
        TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => Vec::new(),
    };
    let written: Vec<Local> = terminator.written_places().map(|p| p.local).collect();
    (handed.iter())
        .filter_map(|operand| operand.place())
        .flat_map(|from| written.iter().map(move |into| (from.local, *into)))
        .collect()
}

/// Why `local`, with `aliases`, holds no reference of its own that only its
/// aliases reach, if it does not.
fn uncounted_root(body: &Body, local: Local, aliases: &HashSet<Local>) -> Option<UncountedRoot> {
    let is_counted = (body.local_decls.get(local.0)).is_some_and(|d| body.is_counted(&d.ty.kind));
    if !is_counted {
        return Some(UncountedRoot::NotCounted);
    }
    let address_taken = address_taken_locals(body);
    let mut sorted: Vec<Local> = aliases.iter().copied().collect();
    sorted.sort_by_key(|alias| alias.0);
    sorted
        .into_iter()
        .find_map(|alias| alias_failure(body, alias, &address_taken))
        .or_else(|| {
            uncounted_definition(body, local, &address_taken)
                .map(UncountedRoot::UncountedDefinition)
        })
}

/// Why the alias `alias` may let code this body does not show reach the
/// collection, if it may.
fn alias_failure(
    body: &Body,
    alias: Local,
    address_taken: &HashSet<Local>,
) -> Option<UncountedRoot> {
    let is_param = (1..=body.arg_count).contains(&alias.0);
    if is_param && !(body.is_owned_param(alias) && body.is_lent_param(alias)) {
        return Some(UncountedRoot::Parameter(alias));
    }
    holder_failure(body, alias, address_taken)
}

/// Why code this body does not show may reach the value `local` holds
/// whatever its parameter status: it is borrowed, captured, gpu-resident or
/// address-taken.
fn holder_failure(
    body: &Body,
    local: Local,
    address_taken: &HashSet<Local>,
) -> Option<UncountedRoot> {
    if body.borrowed_locals.contains_key(&local) {
        return Some(UncountedRoot::Borrowed(local));
    }
    if body.env_capture_locals.contains(&local) {
        return Some(UncountedRoot::Captured(local));
    }
    let is_resident = (body.local_decls.get(local.0)).is_some_and(|d| d.device_handle.is_some());
    if is_resident {
        return Some(UncountedRoot::DeviceResident(local));
    }
    address_taken
        .contains(&local)
        .then_some(UncountedRoot::AddressTaken(local))
}

/// The root of every place whose address `body` takes.
fn address_taken_locals(body: &Body) -> HashSet<Local> {
    (body.basic_blocks.iter())
        .flat_map(|data| &data.statements)
        .filter_map(|statement| match &statement.kind {
            StatementKind::Assign(_, Rvalue::Ref(place))
            | StatementKind::Reassign(_, Rvalue::Ref(place)) => Some(place.local),
            StatementKind::Assign(..)
            | StatementKind::Reassign(..)
            | StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::Nop => None,
        })
        .collect()
}

/// The first block giving the whole of `local` a value it holds no
/// reference to, if any does. A call's result, an aggregate just built, a
/// value moved out of a local holding its own reference, and a copy of a
/// place the block retained first each hand it one; a call writes only
/// counted values. `address_taken` is the root of every place whose
/// address `body` takes.
fn uncounted_definition(
    body: &Body,
    local: Local,
    address_taken: &HashSet<Local>,
) -> Option<BasicBlock> {
    body.basic_blocks
        .iter()
        .enumerate()
        .find(|(_, data)| {
            (data.statements.iter().enumerate()).any(|(position, statement)| {
                let statements = &data.statements;
                is_uncounted_definition(body, statements, position, statement, local, address_taken)
            })
        })
        .map(|(index, _)| BasicBlock(index))
}

/// Whether `statement`, at `position` of `statements`, gives the whole of
/// `local` a value it holds no reference to; `address_taken` is the root of
/// every place whose address `body` takes.
fn is_uncounted_definition(
    body: &Body,
    statements: &[Statement],
    position: usize,
    statement: &Statement,
    local: Local,
    address_taken: &HashSet<Local>,
) -> bool {
    let (StatementKind::Assign(written, rvalue) | StatementKind::Reassign(written, rvalue)) =
        &statement.kind
    else {
        return false;
    };
    if written.local != local || !written.projection.is_empty() {
        return false;
    }
    match rvalue {
        Rvalue::Aggregate(..) => false,
        Rvalue::Use(Operand::Move(source)) if is_owning_local(body, source, address_taken) => false,
        Rvalue::Use(Operand::Copy(source) | Operand::Move(source)) => {
            !is_retained_before(statements, position, source)
        }
        Rvalue::Use(Operand::Constant(_) | Operand::Function(_))
        | Rvalue::Ref(_)
        | Rvalue::BinaryOp(..)
        | Rvalue::UnaryOp(..)
        | Rvalue::Cast(..)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(..)
        | Rvalue::AtomicOp { .. } => true,
    }
}

/// Whether `place` is a whole local holding a reference of its own that
/// only this body reaches: not a parameter the body does not take over, and
/// not borrowed, captured, gpu-resident or in `address_taken`.
fn is_owning_local(body: &Body, place: &Place, address_taken: &HashSet<Local>) -> bool {
    let is_param = (1..=body.arg_count).contains(&place.local.0);
    place.projection.is_empty()
        && (!is_param || body.is_owned_param(place.local))
        && holder_failure(body, place.local, address_taken).is_none()
}

/// Whether a statement of `statements` before `position` retains `place`,
/// with nothing between writing its root.
fn is_retained_before(statements: &[Statement], position: usize, place: &Place) -> bool {
    let earlier = statements.get(..position).unwrap_or_default();
    earlier
        .iter()
        .rev()
        .take_while(|statement| {
            statement
                .written_place()
                .is_none_or(|written| written.local != place.local)
        })
        .any(|statement| matches!(&statement.kind, StatementKind::IncRef(retained) if retained == place))
}
