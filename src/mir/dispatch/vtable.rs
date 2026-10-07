// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Vtables as data: the shared slot numbering, the vtable an instance points
//! at, and the slots each vtable is filled with.

use crate::ast::types::{Type, TypeKind};
use crate::mir::instantiation::monomorphized_arguments;
use crate::mir::symbol::{Symbol, ThunkKind};
use crate::mir::{AggregateKind, Body, Rvalue, StatementKind};
use crate::type_checker::context::{
    class_ancestry, class_is_extended, class_needs_vtable, ClassDefinition, MethodInfo,
    TypeDefinition,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// The vtable one constructed class instance points at: the class, and the
/// type arguments it is built at when they have a monomorphized spelling.
///
/// A generic class constructed where its arguments have no spelling carries
/// none, and its vtable is the class's bare one, whose slots name the bodies
/// shared by every instantiation, as a static call at those open arguments
/// does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VtableInstance {
    class: String,
    args: Vec<Type>,
}

impl VtableInstance {
    /// The vtable an instance built at `instance_ty` points at, or `None` when
    /// the type names no class that takes part in virtual dispatch.
    ///
    /// The arguments are read as a static call on the instance reads them,
    /// so the vtable and the call name one instantiation.
    ///
    /// Only an instance built at open arguments has none to spell, and it
    /// points at the bare vtable: one built inside a body shared by every
    /// instantiation of its enclosing declaration — `let o Op<T> = Impl<T>()`
    /// in a method of `class Wrapper<T>`, or in a generic function whose
    /// parameter appears only in its return type. An instance built in a body
    /// lowered for one instantiation always spells its arguments, since
    /// constructor lowering refuses one that does not
    /// ([`LoweringContext::refuse_unnameable_instance`]).
    ///
    /// [`LoweringContext::refuse_unnameable_instance`]: crate::mir::lowering::context::LoweringContext::refuse_unnameable_instance
    pub fn of(instance_ty: &Type, type_defs: &HashMap<String, TypeDefinition>) -> Option<Self> {
        let TypeKind::Custom(class, arg_exprs) = &instance_ty.kind else {
            return None;
        };
        let Some(TypeDefinition::Class(class_def)) = type_defs.get(class.as_str()) else {
            return None;
        };
        if !class_needs_vtable(class, type_defs) {
            return None;
        }
        let arity = class_def.generics.as_ref().map_or(0, Vec::len);
        let args = arg_exprs
            .as_deref()
            .and_then(|exprs| monomorphized_arguments(exprs, arity, type_defs))
            .unwrap_or_default();
        Some(Self {
            class: class.clone(),
            args,
        })
    }

    /// The class the instance is built of.
    pub fn class(&self) -> &str {
        &self.class
    }

    /// The type arguments the instance is built at, none for the bare vtable.
    pub fn args(&self) -> &[Type] {
        &self.args
    }

    /// The data symbol of this vtable: `miri.{class}.$vtable`, mangled by the
    /// instantiation's arguments as every other per-instantiation symbol is.
    pub fn symbol(&self) -> String {
        Symbol::vtable(&self.class, &self.args).link_name()
    }

    /// The symbol of the drop thunk that releases an instance of this vtable's
    /// class at its arguments, which [`DROP_SLOT`] points at: the thunk of the
    /// instantiation for one built at arguments, the shared one otherwise.
    pub fn drop_symbol(&self) -> String {
        Symbol::type_thunk(ThunkKind::Drop, &self.class, self.args.iter()).link_name()
    }
}

/// Every vtable an instance constructed in `bodies` points at, in order of
/// symbol: its symbol, and the drop thunk its [`DROP_SLOT`] points at.
pub fn constructed_vtable_symbols<'b>(
    bodies: impl IntoIterator<Item = &'b Body>,
    type_defs: &HashMap<String, TypeDefinition>,
) -> BTreeMap<String, String> {
    bodies
        .into_iter()
        .flat_map(|body| &body.basic_blocks)
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            StatementKind::Assign(_, rvalue) | StatementKind::Reassign(_, rvalue) => {
                constructed_class(rvalue)
            }
            StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::Nop
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_) => None,
        })
        .filter_map(|ty| VtableInstance::of(ty, type_defs))
        .map(|instance| (instance.symbol(), instance.drop_symbol()))
        .collect()
}

/// The type of the class instance `rvalue` constructs, if it constructs one.
pub(crate) fn constructed_class(rvalue: &Rvalue) -> Option<&Type> {
    let Rvalue::Aggregate(AggregateKind::Class(ty), _) = rvalue else {
        return None;
    };
    Some(ty)
}

/// The slot numbering every vtable in the program shares.
///
/// Slot [`DROP_SLOT`] holds the routine that releases an instance of the
/// vtable's own class, which a value held at a base class or a trait is
/// released through. Every other slot stands for one method name: the sorted
/// set of every instance method a trait or an abstract class declares, and of
/// every one a class overrides from a class it extends, constructors and
/// statics aside. A concrete method no subclass overrides takes no slot, and
/// a call to it stays static. Every vtable has one slot per name, filled
/// where the class gives that method a body and null elsewhere; a class has
/// one method per name, so a call through any trait or base class it is
/// reached by finds its own body at the one index the method's name takes.
///
/// The numbering depends on method names alone, which registering a generic
/// instantiation never adds to, so one layout serves a whole compilation.
#[derive(Debug)]
pub struct VtableLayout {
    selectors: Vec<Box<str>>,
}

impl VtableLayout {
    /// The numbering the traits, abstract classes and overrides of
    /// `type_defs` give.
    pub fn of(type_defs: &HashMap<String, TypeDefinition>) -> Self {
        let declared = type_defs
            .values()
            .filter_map(dispatching_methods)
            .flat_map(dispatched_method_names);
        let selectors: BTreeSet<&str> =
            declared.chain(overridden_method_names(type_defs)).collect();
        Self {
            selectors: selectors.into_iter().map(Box::from).collect(),
        }
    }

    /// The number of slots in every vtable, the drop slot included.
    pub fn slot_count(&self) -> usize {
        self.selectors.len() + 1
    }

    /// The slot `method_name` takes, or `None` when no trait or abstract class
    /// declares it and no class overrides it.
    pub fn slot(&self, method_name: &str) -> Option<usize> {
        self.selectors
            .binary_search_by(|selector| (**selector).cmp(method_name))
            .ok()
            .map(|index| index + 1)
    }
}

/// The vtable slot holding the routine that releases an instance of the
/// vtable's class: its drop hook, its managed fields, its allocation.
pub const DROP_SLOT: usize = 0;

/// Whether a value whose static type is `name` may be an instance of another
/// class, and so has to be released as its runtime class through the drop
/// slot of the vtable it points at: a trait, or a class another class
/// extends. Every class in a chain carries a vtable ([`class_needs_vtable`]).
pub fn released_by_runtime_class(name: &str, type_defs: &HashMap<String, TypeDefinition>) -> bool {
    match type_defs.get(name) {
        Some(TypeDefinition::Trait(_)) => true,
        Some(TypeDefinition::Class(_)) => class_is_extended(name, type_defs),
        Some(
            TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_),
        )
        | None => false,
    }
}

/// The methods of a definition whose instance methods take vtable slots: a
/// trait's, or an abstract class's.
fn dispatching_methods(definition: &TypeDefinition) -> Option<&BTreeMap<String, MethodInfo>> {
    match definition {
        TypeDefinition::Trait(trait_def) => Some(&trait_def.methods),
        TypeDefinition::Class(class) => class.is_abstract.then_some(&class.methods),
        TypeDefinition::Struct(_)
        | TypeDefinition::Enum(_)
        | TypeDefinition::Generic(_)
        | TypeDefinition::Alias(_) => None,
    }
}

/// The name of every method some class overrides: one it declares that a
/// class it extends, directly or further up, declares too.
pub(crate) fn overridden_method_names(
    type_defs: &HashMap<String, TypeDefinition>,
) -> impl Iterator<Item = &str> {
    type_defs
        .values()
        .filter_map(|definition| match definition {
            TypeDefinition::Class(class) => Some(class),
            TypeDefinition::Trait(_)
            | TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_) => None,
        })
        .flat_map(move |class| overrides_of(class, type_defs))
}

/// The methods `class` declares that a class it extends declares too.
fn overrides_of<'td>(
    class: &'td ClassDefinition,
    type_defs: &'td HashMap<String, TypeDefinition>,
) -> impl Iterator<Item = &'td str> {
    let ancestors: Vec<&ClassDefinition> = class
        .base_class
        .as_deref()
        .map(|base| {
            class_ancestry(base, type_defs)
                .map(|(_, def)| def)
                .collect()
        })
        .unwrap_or_default();
    dispatched_method_names(&class.methods).filter(move |method| {
        ancestors
            .iter()
            .any(|ancestor| ancestor.methods.get(*method).is_some_and(takes_vtable_slot))
    })
}

/// The names among `methods` a vtable slot can stand for.
pub(crate) fn dispatched_method_names(
    methods: &BTreeMap<String, MethodInfo>,
) -> impl Iterator<Item = &str> {
    methods
        .iter()
        .filter(|(_, info)| takes_vtable_slot(info))
        .map(|(name, _)| name.as_str())
}

/// Whether a method takes a vtable slot: every one but the constructors and
/// statics.
pub(crate) fn takes_vtable_slot(info: &MethodInfo) -> bool {
    !info.is_constructor && !info.is_static
}

/// One filled vtable slot: its number, the method it stands for and the
/// symbol of the body it points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilledSlot {
    pub slot: usize,
    pub method: String,
    pub symbol: String,
}

/// The filled slots of every vtable a program's reached bodies build, by
/// vtable symbol: what codegen writes into each vtable it defines.
#[derive(Debug, Default, Clone)]
pub struct VtableFills {
    slots: BTreeMap<String, Vec<FilledSlot>>,
}

impl VtableFills {
    /// The filled slots of the vtable `symbol`, in the order they were
    /// filled; none for a vtable no reached body builds.
    pub fn slots(&self, symbol: &str) -> &[FilledSlot] {
        self.slots.get(symbol).map_or(&[], Vec::as_slice)
    }
}

impl FromIterator<(String, Vec<FilledSlot>)> for VtableFills {
    /// Fills keyed by vtable symbol, each vtable's slots in the order they
    /// were filled.
    fn from_iter<I: IntoIterator<Item = (String, Vec<FilledSlot>)>>(iter: I) -> Self {
        Self {
            slots: iter.into_iter().collect(),
        }
    }
}
