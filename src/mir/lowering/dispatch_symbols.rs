// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The function symbols a class's dispatch resolves to where no MIR call spells
//! them: the slots of its vtable, the drop hook releasing it runs, the `clone`
//! a container copies it through and the methods a container asks of its
//! elements. Codegen emits those references and the pipeline compiles every
//! body they name, so both read them from here.
//!
//! A vtable belongs to one instantiation of a class: the one a constructor
//! builds the instance at, which [`VtableInstance`] names. Its slots name what
//! a static call on that instantiation names, so a call through a trait
//! receiver reaches the body compiled at the instance's own type arguments.
//!
//! This is also where the rule for which trait default a class inherits
//! lives, and what a copy of one compiled under the class's name is typed at:
//! static dispatch, vtable slots and the per-class copies the pipeline lowers
//! all choose by [`trait_default_among`]. A container's equality thunk does
//! not yet: it asks only the class chain for `equals`, so an `equals` a class
//! inherits from a trait default is not what a set or map matches by.

use super::method_dispatch::{instantiated_callee, resolve_inherited_method};
use super::monomorphized_arguments;
use crate::ast::statement::DROP_HOOK_NAME;
use crate::ast::types::{
    Type, TypeKind, CLONE_METHOD_NAME, EQUALS_METHOD_NAME, ORDERING_METHOD_NAME,
};
use crate::mir::symbol::Symbol;
use crate::mir::{AggregateKind, Body, Rvalue, StatementKind};
use crate::type_checker::context::{
    class_needs_vtable, find_trait_default_method, ClassDefinition, MethodInfo, TraitDefinition,
    TypeDefinition,
};
use crate::type_checker::utils::has_drop_hook;
use crate::type_checker::TypeChecker;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// The methods a container's runtime thunk asks of two class elements: the
/// ordering it sorts by and the equality it matches by.
pub const ELEMENT_METHOD_NAMES: [&str; 2] = [ORDERING_METHOD_NAME, EQUALS_METHOD_NAME];

/// The methods a runtime thunk calls on a class instance, besides its drop hook.
const THUNK_METHOD_NAMES: [&str; 3] = {
    let [ordering, equals] = ELEMENT_METHOD_NAMES;
    [CLONE_METHOD_NAME, ordering, equals]
};

/// The first of `traits`, in the order a class lists them, to supply a default
/// `method_name` — itself or through a parent trait — paired with the trait
/// that declares the default and its signature.
pub fn trait_default_among<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    traits: &'td [String],
    method_name: &str,
) -> Option<(&'td str, &'td MethodInfo)> {
    traits
        .iter()
        .find_map(|trait_name| find_trait_default_method(type_defs, trait_name, method_name))
}

/// The trait whose default `method_name` a `class_name` receiver inherits: the
/// class's own traits by [`trait_default_among`], then each base's in turn.
pub fn inherited_trait_default<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    class_name: &str,
    method_name: &str,
) -> Option<&'td str> {
    class_chain(type_defs, class_name)
        .find_map(|(_, class)| trait_default_among(type_defs, &class.traits, method_name))
        .map(|(trait_name, _)| trait_name)
}

/// The substitution the copy of `method_name` compiled under `class_name` for
/// one instantiation reads its body through, given the class's own parameters
/// at that instantiation.
///
/// A method the class declares reads `class_substitution` as it is. A trait
/// default it inherits reads it with the trait's parameters laid over it at
/// what the chain pins them to, as
/// [`TypeChecker::trait_default_substitution`] states.
pub fn instantiation_substitution<'s>(
    type_checker: &TypeChecker,
    class_name: &str,
    method_name: &str,
    class_substitution: &'s HashMap<String, Type>,
) -> Cow<'s, HashMap<String, Type>> {
    let type_defs = type_checker.type_definitions();
    let is_declared = matches!(
        type_defs.get(class_name),
        Some(TypeDefinition::Class(class)) if class.methods.contains_key(method_name)
    );
    let default_owner = (!is_declared)
        .then(|| inherited_trait_default(type_defs, class_name, method_name))
        .flatten();
    match default_owner {
        Some(trait_name) => Cow::Owned(type_checker.trait_default_substitution(
            class_name,
            trait_name,
            class_substitution,
        )),
        None => Cow::Borrowed(class_substitution),
    }
}

/// Every method whose shared body can be compiled under `class_name`'s own
/// name: the ones it or an ancestor declares — an abstract ancestor's are
/// re-lowered per concrete class — and the trait defaults it inherits.
pub fn methods_compiled_under<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    class_name: &str,
) -> impl Iterator<Item = &'td str> {
    let declared = class_chain(type_defs, class_name)
        .flat_map(|(_, class)| class.methods.keys().map(String::as_str));
    let defaulted = inherited_trait_defaults(type_defs, class_name)
        .into_iter()
        .map(|(method, _)| method);
    declared.chain(defaulted)
}

/// Every method a trait anywhere in `class_name`'s chain gives a default body
/// that no class in the chain declares, each paired with the trait
/// [`inherited_trait_default`] chooses, sorted by method name.
pub fn inherited_trait_defaults<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    class_name: &str,
) -> Vec<(&'td str, &'td str)> {
    let chain: Vec<&ClassDefinition> = class_chain(type_defs, class_name)
        .map(|(_, class)| class)
        .collect();
    let traits = chain.iter().flat_map(|class| class.traits.iter());
    defaulted_method_names(type_defs, traits)
        .into_iter()
        .filter(|method| {
            !chain
                .iter()
                .any(|class| class.methods.contains_key(*method))
        })
        .filter_map(|method| {
            inherited_trait_default(type_defs, class_name, method).map(|owner| (method, owner))
        })
        .collect()
}

/// Every method `class_name`'s own trait clauses give a default body that the
/// class does not declare itself, each paired with the trait
/// [`trait_default_among`] chooses, sorted by method name. These are the
/// defaults compiled under the class's own name per instantiation.
pub fn own_trait_defaults<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    class_name: &str,
) -> Vec<(&'td str, &'td str)> {
    let Some(TypeDefinition::Class(class)) = type_defs.get(class_name) else {
        return Vec::new();
    };
    defaulted_method_names(type_defs, class.traits.iter())
        .into_iter()
        .filter(|method| !class.methods.contains_key(*method))
        .filter_map(|method| {
            trait_default_among(type_defs, &class.traits, method).map(|(owner, _)| (method, owner))
        })
        .collect()
}

/// The names of every default body `traits` and their parent traits declare.
fn defaulted_method_names<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    traits: impl Iterator<Item = &'td String>,
) -> BTreeSet<&'td str> {
    trait_hierarchy(type_defs, traits.map(String::as_str))
        .flat_map(|trait_def| trait_def.methods.iter())
        .filter(|(_, method)| !method.is_abstract)
        .map(|(name, _)| name.as_str())
        .collect()
}

/// Every trait `roots` name and every parent trait they extend, each once.
/// A name that registers no trait is passed over.
fn trait_hierarchy<'td, 'n>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    roots: impl Iterator<Item = &'n str>,
) -> impl Iterator<Item = &'td TraitDefinition> {
    let mut pending: Vec<&'td str> = roots
        .filter_map(|name| type_defs.get_key_value(name))
        .map(|(name, _)| name.as_str())
        .collect();
    let mut visited = HashSet::new();
    std::iter::from_fn(move || loop {
        let trait_name = pending.pop()?;
        if !visited.insert(trait_name) {
            continue;
        }
        let Some(TypeDefinition::Trait(trait_def)) = type_defs.get(trait_name) else {
            continue;
        };
        pending.extend(trait_def.parent_traits.iter().map(String::as_str));
        return Some(trait_def);
    })
}

/// `class_name` and every class it extends, nearest first, each with its name.
///
/// Bounded by the number of definitions, so a circular `extends` — reported
/// where the class is declared — cannot hang the walk.
fn class_chain<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    class_name: &str,
) -> impl Iterator<Item = (&'td str, &'td ClassDefinition)> {
    let mut next = class_entry(type_defs, class_name);
    std::iter::from_fn(move || {
        let (name, class) = next?;
        next = class
            .base_class
            .as_deref()
            .and_then(|base| class_entry(type_defs, base));
        Some((name, class))
    })
    .take(type_defs.len() + 1)
}

/// The class registered under `name`, with the name as the table holds it.
fn class_entry<'td>(
    type_defs: &'td HashMap<String, TypeDefinition>,
    name: &str,
) -> Option<(&'td str, &'td ClassDefinition)> {
    let (name, TypeDefinition::Class(class)) = type_defs.get_key_value(name)? else {
        return None;
    };
    Some((name.as_str(), class))
}

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
    /// [`LoweringContext::refuse_unnameable_instance`]: super::context::LoweringContext::refuse_unnameable_instance
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

    /// The data symbol of this vtable: `__vtable_{class}`, mangled by the
    /// instantiation's arguments as every other per-instantiation symbol is.
    pub fn symbol(&self) -> String {
        Symbol::vtable(&self.class, &self.args).link_name()
    }

    /// Every method this vtable has a slot for, with the symbol the slot names
    /// when it is filled, in the order [`collect_vtable_methods`] lists them.
    pub fn slot_targets<'td>(
        &self,
        type_defs: &'td HashMap<String, TypeDefinition>,
    ) -> Vec<(&'td str, Option<String>)> {
        collect_vtable_methods(&self.class, type_defs)
            .into_iter()
            .map(|method| (method, self.slot_target(method, type_defs)))
            .collect()
    }

    /// The symbol this vtable's slot for `method_name` names: the body a
    /// static call on the instantiation names where one is compiled per
    /// instantiation, else what [`resolve_vtable_method`] resolves for the
    /// class. `None` when nothing in the chain gives the method a body.
    fn slot_target(
        &self,
        method_name: &str,
        type_defs: &HashMap<String, TypeDefinition>,
    ) -> Option<String> {
        instantiated_callee(type_defs, &self.class, &self.args, method_name)
            .map(|callee| callee.symbol)
            .or_else(|| resolve_vtable_method(&self.class, method_name, type_defs))
    }
}

/// The symbol of every vtable an instance constructed in `bodies` points at,
/// in order.
pub fn constructed_vtable_symbols<'b>(
    bodies: impl IntoIterator<Item = &'b Body>,
    type_defs: &HashMap<String, TypeDefinition>,
) -> BTreeSet<String> {
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
        .map(|instance| instance.symbol())
        .collect()
}

/// The type of the class instance `rvalue` constructs, if it constructs one.
pub(crate) fn constructed_class(rvalue: &Rvalue) -> Option<&Type> {
    let Rvalue::Aggregate(AggregateKind::Class(ty), _) = rvalue else {
        return None;
    };
    Some(ty)
}

/// The method names of `class_name`'s filled vtable slots: every method its
/// abstract ancestors declare and every method a trait in its chain requires,
/// constructors and statics aside, sorted by name.
///
/// Where each lands in the vtable is [`VtableLayout::slot`]'s to say.
pub fn collect_vtable_methods<'td>(
    class_name: &str,
    type_defs: &'td HashMap<String, TypeDefinition>,
) -> Vec<&'td str> {
    let chain: Vec<&ClassDefinition> = class_chain(type_defs, class_name)
        .map(|(_, class)| class)
        .collect();
    let abstract_methods = chain
        .iter()
        .filter(|class| class.is_abstract)
        .flat_map(|class| dispatched_method_names(&class.methods));
    let traits = chain
        .iter()
        .flat_map(|class| class.traits.iter().map(String::as_str));
    let trait_methods = trait_hierarchy(type_defs, traits)
        .flat_map(|trait_def| dispatched_method_names(&trait_def.methods));
    let methods: BTreeSet<&str> = abstract_methods.chain(trait_methods).collect();
    methods.into_iter().collect()
}

/// The slot numbering every vtable in the program shares.
///
/// A slot stands for one method name: the sorted set of every instance method
/// a trait or an abstract class declares, constructors and statics aside.
/// Every vtable has one slot per name, filled where the class gives that
/// method a body and null elsewhere; a class has one method per name, so a
/// call through any trait or abstract base it is reached by finds its own
/// body at the one index the method's name takes.
///
/// The numbering depends on method names alone, which registering a generic
/// instantiation never adds to, so one layout serves a whole compilation.
#[derive(Debug)]
pub struct VtableLayout {
    selectors: Vec<Box<str>>,
}

impl VtableLayout {
    /// The numbering the traits and abstract classes of `type_defs` give.
    pub fn of(type_defs: &HashMap<String, TypeDefinition>) -> Self {
        let selectors: BTreeSet<&str> = type_defs
            .values()
            .filter_map(dispatching_methods)
            .flat_map(dispatched_method_names)
            .collect();
        Self {
            selectors: selectors.into_iter().map(Box::from).collect(),
        }
    }

    /// The number of slots in every vtable.
    pub fn slot_count(&self) -> usize {
        self.selectors.len()
    }

    /// The slot `method_name` takes, or `None` when no trait or abstract class
    /// declares it.
    pub fn slot(&self, method_name: &str) -> Option<usize> {
        self.selectors
            .binary_search_by(|selector| (**selector).cmp(method_name))
            .ok()
    }
}

/// The slot of `layout` a call to `method_name` through a `receiver` — a
/// trait or an abstract class — reads, or `None` when the receiver dispatches
/// no such method and the call is static.
pub fn vtable_slot_index(
    layout: &VtableLayout,
    receiver: &str,
    method_name: &str,
    type_defs: &HashMap<String, TypeDefinition>,
) -> Option<usize> {
    dispatches_through_vtable(receiver, method_name, type_defs)
        .then(|| layout.slot(method_name))
        .flatten()
}

/// Whether `receiver` reaches `method_name` through its vtable: a trait whose
/// hierarchy declares it, or an abstract class whose abstract ancestors or
/// whose chain's traits do.
fn dispatches_through_vtable(
    receiver: &str,
    method_name: &str,
    type_defs: &HashMap<String, TypeDefinition>,
) -> bool {
    match type_defs.get(receiver) {
        Some(TypeDefinition::Trait(_)) => {
            traits_dispatch(type_defs, std::iter::once(receiver), method_name)
        }
        Some(TypeDefinition::Class(class)) if class.is_abstract => class_chain(type_defs, receiver)
            .any(|(_, class)| {
                (class.is_abstract && declares_dispatched(&class.methods, method_name))
                    || traits_dispatch(
                        type_defs,
                        class.traits.iter().map(String::as_str),
                        method_name,
                    )
            }),
        Some(
            TypeDefinition::Class(_)
            | TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_),
        )
        | None => false,
    }
}

/// Whether a trait `roots` names, or a parent trait of one, declares
/// `method_name` as one a vtable slot stands for.
fn traits_dispatch<'n>(
    type_defs: &HashMap<String, TypeDefinition>,
    roots: impl Iterator<Item = &'n str>,
    method_name: &str,
) -> bool {
    trait_hierarchy(type_defs, roots)
        .any(|trait_def| declares_dispatched(&trait_def.methods, method_name))
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

/// The names among `methods` a vtable slot can stand for.
fn dispatched_method_names(methods: &BTreeMap<String, MethodInfo>) -> impl Iterator<Item = &str> {
    methods
        .iter()
        .filter(|(_, info)| takes_vtable_slot(info))
        .map(|(name, _)| name.as_str())
}

/// Whether `methods` declares `method_name` as one a vtable slot stands for.
fn declares_dispatched(methods: &BTreeMap<String, MethodInfo>, method_name: &str) -> bool {
    methods.get(method_name).is_some_and(takes_vtable_slot)
}

/// Whether a method takes a vtable slot: every one but the constructors and
/// statics.
fn takes_vtable_slot(info: &MethodInfo) -> bool {
    !info.is_constructor && !info.is_static
}

/// The symbol the slot for `method_name` names in `class_name`'s vtable where
/// no per-instantiation body applies: the nearest class in its chain that
/// gives the method a body, else the trait default [`inherited_trait_default`]
/// chooses.
///
/// A default a class without type parameters of its own inherits resolves to
/// the class's own copy, `"{class_name}_{method_name}"`: the pipeline lowers
/// one for every concrete class that declares the method nowhere in its chain,
/// typed at what its clauses pin the trait's parameters to, and a static call
/// on the class names the same body. A generic class reaches this only through
/// its bare vtable; its own copy there leaves its parameters open, so its slot
/// — like one whose chain declares the method abstractly — names the trait's
/// shared `"{Trait}_{method_name}"`.
///
/// A generic class's bare slots are what an instance runs when its arguments
/// could not be spelled where it was built, as [`VtableInstance::of`] lists.
/// The shared body they name reads a type parameter as an unmanaged word, so a
/// default
/// that overwrites a managed `T` local double-frees it: a construction inside
/// a body shared by every instantiation of its enclosing declaration is not
/// specialized per instantiation, and neither is its instance's vtable.
// TODO: an instance constructed inside a shared generic body — a method of
// `class Wrapper<T>` doing `let o Op<T> = Impl<T>()`, a closure building
// `Impl<T>`, a generic function whose parameter appears only in its return
// type — reaches these bare slots at every instantiation, and a trait default
// reassigning a `var x T` there double-frees a managed argument. Settling it
// needs the enclosing body compiled per instantiation, so the construction
// inside it spells its arguments.
pub fn resolve_vtable_method(
    class_name: &str,
    method_name: &str,
    type_defs: &HashMap<String, TypeDefinition>,
) -> Option<String> {
    let mut is_declared_in_chain = false;
    for (name, class) in class_chain(type_defs, class_name) {
        if let Some(method) = class.methods.get(method_name) {
            if !method.is_abstract {
                return Some(Symbol::method(name, &[], method_name, &[]).link_name());
            }
            is_declared_in_chain = true;
        }
    }
    let defining_trait = inherited_trait_default(type_defs, class_name, method_name)?;
    let is_generic_class = matches!(
        type_defs.get(class_name),
        Some(TypeDefinition::Class(class)) if class.generics.is_some()
    );
    let owner = if is_declared_in_chain || is_generic_class {
        defining_trait
    } else {
        class_name
    };
    Some(Symbol::method(owner, &[], method_name, &[]).link_name())
}

/// The symbol of the body a call to `method_name` on a `type_name` receiver
/// names, resolved as static dispatch resolves it, or `None` when nothing in
/// the type's chain declares the method.
pub fn inherited_method_symbol(
    type_defs: &HashMap<String, TypeDefinition>,
    type_name: &str,
    method_name: &str,
) -> Option<String> {
    resolve_inherited_method(type_defs, type_name, method_name)
        .map(|(owner, _)| Symbol::method(&owner, &[], method_name, &[]).link_name())
}

/// The symbol a call to `method_name` on a `type_name` receiver names:
/// [`inherited_method_symbol`] where the chain declares the method, else the
/// type's own `"{type_name}_{method_name}"`.
pub fn method_symbol(
    type_defs: &HashMap<String, TypeDefinition>,
    type_name: &str,
    method_name: &str,
) -> String {
    inherited_method_symbol(type_defs, type_name, method_name)
        .unwrap_or_else(|| Symbol::method(type_name, &[], method_name, &[]).link_name())
}

/// The symbol of the drop hook releasing a `type_name` value runs, or `None`
/// when the type has none. A class reaches the hook its chain declares or
/// inherits from a trait; a struct names its own.
pub fn drop_hook_symbol(
    type_name: &str,
    type_defs: &HashMap<String, TypeDefinition>,
) -> Option<String> {
    if !has_drop_hook(type_name, type_defs) {
        return None;
    }
    Some(method_symbol(type_defs, type_name, DROP_HOOK_NAME))
}

/// The symbol of the `clone` a copy of a `type_name` element calls.
pub fn clone_method_symbol(type_name: &str, type_defs: &HashMap<String, TypeDefinition>) -> String {
    method_symbol(type_defs, type_name, CLONE_METHOD_NAME)
}

/// Every function symbol codegen names without a MIR call carrying it, for
/// every class: the drop hook and the `clone` and element methods its runtime
/// thunks call. A vtable slot is named only where a reached body constructs an
/// instance and a reached virtual call reads it, so
/// [`super::vtable_demand::VtableDemand`] reads those off the lowered bodies.
///
/// A thunk's own predicate can still skip a class, so this may name a body no
/// thunk ends up calling; that costs one body compiled needlessly, never one
/// missing at link time.
pub fn synthesized_references(type_defs: &HashMap<String, TypeDefinition>) -> HashSet<String> {
    let mut symbols = HashSet::new();
    for (type_name, definition) in type_defs {
        let TypeDefinition::Class(_) = definition else {
            continue;
        };
        symbols.extend(drop_hook_symbol(type_name, type_defs));
        symbols.extend(
            THUNK_METHOD_NAMES
                .iter()
                .filter_map(|method| inherited_method_symbol(type_defs, type_name, method)),
        );
    }
    symbols
}
