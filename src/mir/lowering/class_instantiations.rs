// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Finding the generic instantiations a type spells.
//!
//! The pipeline fills the instantiation registry from the types the checker
//! inferred. A body lowered for one instantiation of a generic function or
//! class names further instantiations the checker never saw concretely —
//! `Box<T>` inside `via_box<T>` becomes `Box<String>` only once `via_box` is
//! lowered at `String` — and records them on the body so the pipeline can add
//! them too.
//!
//! Every instantiation added that way passes the bounds of
//! [`super::instantiation_limits`] first, and a growing chain of them is
//! followed deepest first, so a program that grows without end reaches the
//! depth bound after one instantiation per level rather than after every
//! combination of the levels below it.

use super::context::LoweringContext;
use super::dispatch_symbols::constructed_class;
use super::instantiation_argument;
use super::instantiation_limits::{
    constructor_parts, exceeded_limit, has_value_argument, instance_type_depth,
    invalid_value_argument, mentions_open_parameter, polymorphic_recursion,
    unnameable_type_argument, value_growth_note, ExceededLimit, Growth,
};
use crate::ast::expression::Expression;
use crate::ast::types::{Type, TypeKind};
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::body::GenericClassInstantiation;
use crate::mir::symbol::{Symbol, TypeInstance};
use crate::mir::{Body, StatementKind};
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::generics::{
    bound_value_names, extract_value_generic, fold_value_generic_arithmetic,
    unfoldable_value_argument, UnfoldableValue,
};
use crate::type_checker::TypeChecker;
use std::collections::{HashMap, HashSet};

/// How deep [`collect_generic_instantiations`] descends through a type's own
/// arguments. Matches the depth the symbol mangler names a type to, past the
/// depth any instance may nest to.
const MAX_INSTANTIATION_NESTING: usize = crate::mir::symbol::token::MAX_TOKEN_DEPTH;

/// Append every generic-class instantiation written inside `kind`, including
/// the ones nested in its own arguments (`List<List<W>>` yields both).
///
/// The descent stops at [`MAX_INSTANTIATION_NESTING`], past which the symbol
/// mangler has no name for the type anyway, so nothing below it could be given
/// a body.
///
/// For the same reason a type spelled at an argument the mangler has no token
/// for is left out, though its arguments are still searched. Inside a generic
/// class `self` has the class at its own parameters (`Box<T>`): that is the
/// generic definition, not an instantiation any call could reach.
///
/// A generic struct and a generic enum are collected alongside a generic class.
/// All three are released through a drop thunk emitted per instantiation, so
/// the field a given instantiation actually stores is the one decremented, and
/// that thunk exists only for an instantiation recorded here. Only the class
/// entries drive method-body monomorphization; those passes select classes
/// themselves.
pub(crate) fn collect_generic_instantiations(
    type_checker: &TypeChecker,
    kind: &TypeKind,
    out: &mut Vec<(String, Vec<Type>)>,
) {
    collect_nested_instantiations(type_checker, kind, 0, out);
}

fn collect_nested_instantiations(
    type_checker: &TypeChecker,
    kind: &TypeKind,
    depth: usize,
    out: &mut Vec<(String, Vec<Type>)>,
) {
    if depth >= MAX_INSTANTIATION_NESTING {
        return;
    }
    let TypeKind::Custom(name, Some(args)) = kind else {
        return;
    };
    let Some(def) = type_checker.type_definitions().get(name.as_str()) else {
        return;
    };
    let Some(generics) = def.generics() else {
        return;
    };
    let resolved: Option<Vec<Type>> = args.iter().map(instantiation_argument).collect();
    let Some(resolved) = resolved else {
        return;
    };
    if resolved.len() != generics.len() {
        return;
    }
    for arg in &resolved {
        collect_nested_instantiations(type_checker, &arg.kind, depth + 1, out);
    }
    if resolved
        .iter()
        .all(|arg| super::has_a_monomorphized_spelling(&arg.kind))
    {
        out.push((name.clone(), resolved));
    }
}

/// Whether the registry already holds `class` at exactly `type_args`.
pub(crate) fn is_registered_instantiation(
    type_checker: &TypeChecker,
    class: &str,
    type_args: &[Type],
) -> bool {
    type_checker
        .generic_class_instantiations
        .get(class)
        .is_some_and(|tuples| tuples.iter().any(|tuple| same_arguments(tuple, type_args)))
}

/// One instantiation a lowered body names that the registry does not hold.
#[derive(Debug, Clone)]
pub struct Unregistered {
    pub class: String,
    pub args: Vec<Type>,
    /// The position of the first lowered body naming it.
    body: usize,
}

/// Every instantiation `bodies` name that the registry does not hold, each
/// once, in the order the bodies first name them.
pub fn unregistered_instantiations(
    type_checker: &TypeChecker,
    bodies: &[(Symbol, Body)],
) -> Vec<Unregistered> {
    let mut seen = HashSet::new();
    let mut found = Vec::new();
    for (index, (_, body)) in bodies.iter().enumerate() {
        for named in &body.generic_class_instantiations {
            if is_registered_instantiation(type_checker, &named.class, &named.type_args)
                || !seen.insert(TypeInstance::new(&named.class, &named.type_args))
            {
                continue;
            }
            found.push(Unregistered {
                class: named.class.clone(),
                args: named.type_args.clone(),
                body: index,
            });
        }
    }
    found
}

/// Refuse the first of `found` that passes a bound of
/// [`super::instantiation_limits`], counting each class's instantiations at
/// value arguments across the registry and all of `found`.
pub fn refuse_past_limits(
    type_checker: &TypeChecker,
    found: &[Unregistered],
    bodies: &[(Symbol, Body)],
) -> Result<(), LoweringError> {
    let type_defs = type_checker.type_definitions();
    let mut value_instances: HashMap<&str, usize> = HashMap::new();
    for instance in found {
        let count = if has_value_argument(&instance.args) {
            let count = value_instances
                .entry(&instance.class)
                .or_insert_with(|| registered_value_instances(type_checker, &instance.class));
            *count += 1;
            *count
        } else {
            0
        };
        if let Some(limit) = exceeded_limit(&instance.class, &instance.args, count, type_defs) {
            return Err(refusal_of(instance, &limit, bodies, type_defs));
        }
    }
    Ok(())
}

/// The refusal of `instance`, which passes `limit`, at the place the first
/// body naming it names it, explained by the chain of instances that built it.
fn refusal_of(
    instance: &Unregistered,
    limit: &ExceededLimit,
    bodies: &[(Symbol, Body)],
    type_defs: &HashMap<String, TypeDefinition>,
) -> LoweringError {
    let (chain, method) = static_growth(instance, bodies, type_defs);
    let growth = Growth {
        chain: chain.iter().map(Vec::as_slice).collect(),
        method: method.as_deref(),
        through_trait: false,
    };
    let span = naming_span(&bodies[instance.body].1, &instance.class, &instance.args);
    polymorphic_recursion(&instance.class, &instance.args, limit, &growth, span)
}

/// Refuse `class` at `args`, an instantiation the declaration at `span`
/// derives from one the registry holds, where it passes a bound of
/// [`super::instantiation_limits`].
pub fn refuse_derived_past_limits(
    type_checker: &TypeChecker,
    class: &str,
    args: &[Type],
    span: Span,
) -> Result<(), LoweringError> {
    if is_registered_instantiation(type_checker, class, args) {
        return Ok(());
    }
    let value_instances = if has_value_argument(args) {
        registered_value_instances(type_checker, class) + 1
    } else {
        0
    };
    match exceeded_limit(
        class,
        args,
        value_instances,
        type_checker.type_definitions(),
    ) {
        Some(limit) => Err(polymorphic_recursion(
            class,
            args,
            &limit,
            &Growth::default(),
            span,
        )),
        None => Ok(()),
    }
}

/// Of `found`, the instantiations to register this round.
///
/// Every one of them, unless one nests deeper than anything the registry
/// holds: then only the deepest, the first of them on a tie. A chain that
/// grows on every level is followed down one instantiation at a time, and
/// reaches the depth bound after one round per level; the ones held back are
/// named again next round, and registered once nothing grows any deeper.
pub fn admitted_this_round(
    type_checker: &TypeChecker,
    found: Vec<Unregistered>,
) -> Vec<Unregistered> {
    let registry_depth = type_checker
        .generic_class_instantiations
        .values()
        .flatten()
        .map(|args| instance_type_depth(args))
        .max()
        .unwrap_or(0);
    let deepest = found
        .iter()
        .enumerate()
        .map(|(index, instance)| (instance_type_depth(&instance.args), index))
        .max_by_key(|&(depth, index)| (depth, std::cmp::Reverse(index)));
    match deepest {
        Some((depth, index)) if depth > registry_depth => {
            found.into_iter().skip(index).take(1).collect()
        }
        Some(_) | None => found,
    }
}

/// How many instantiations of `class` at value arguments the registry holds.
fn registered_value_instances(type_checker: &TypeChecker, class: &str) -> usize {
    type_checker
        .generic_class_instantiations
        .get(class)
        .map_or(0, |tuples| {
            tuples
                .iter()
                .filter(|args| has_value_argument(args))
                .count()
        })
}

/// Where `body` names `class` at `args`: the statement constructing it, else
/// the local declared at a type spelling it, else the body itself.
fn naming_span(body: &Body, class: &str, args: &[Type]) -> Span {
    let constructs = body
        .basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .find(|statement| match &statement.kind {
            StatementKind::Assign(_, rvalue) | StatementKind::Reassign(_, rvalue) => {
                constructed_class(rvalue).is_some_and(|ty| spells_instance(ty, class, args))
            }
            StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::Nop
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_) => false,
        });
    if let Some(statement) = constructs {
        return statement.span;
    }
    body.local_decls
        .iter()
        .find(|decl| spells_instance(&decl.ty, class, args))
        .map_or(body.span, |decl| decl.span)
}

/// Whether `ty` is `class` at exactly `args`.
fn spells_instance(ty: &Type, class: &str, args: &[Type]) -> bool {
    let TypeKind::Custom(name, Some(arg_exprs)) = &ty.kind else {
        return false;
    };
    name == class
        && arg_exprs.len() == args.len()
        && arg_exprs
            .iter()
            .zip(args)
            .all(|(expr, arg)| instantiation_argument(expr).is_some_and(|ty| ty.kind == arg.kind))
}

/// The instances of `instance`'s class whose methods built each next one on
/// the way to it, outermost first, and the method of the last of them.
///
/// A body lowered for a method, or for a generic function taking the class
/// first, runs at the instance its first parameter is, so the instance that
/// built another is the one the first body building it runs at: a body naming
/// the instance other than through its own, or calling the function body that
/// runs at it. The chain ends at an instance no lowered body builds, the one
/// the program wrote. The walk takes at most one step per body, which is
/// enough to reach back through every value instance a class may need.
fn static_growth(
    instance: &Unregistered,
    bodies: &[(Symbol, Body)],
    type_defs: &HashMap<String, TypeDefinition>,
) -> (Vec<Vec<Type>>, Option<String>) {
    let class = instance.class.as_str();
    let builder_of = |args: &[Type], running: Option<&Symbol>| {
        bodies.iter().position(|(symbol, body)| {
            builds_instance(body, class, args)
                || running.is_some_and(|callee| symbol != callee && calls(body, callee))
        })
    };
    let mut chain: Vec<Vec<Type>> = Vec::new();
    let mut method = None;
    let mut builder = builder_of(&instance.args, None).or(Some(instance.body));
    for _ in 0..=bodies.len() {
        let Some(index) = builder else {
            break;
        };
        let (symbol, body) = &bodies[index];
        let Some(owner) = self_instance(body, class) else {
            break;
        };
        if method.is_none() {
            method = method_named(symbol, class, &owner, type_defs);
        }
        builder = builder_of(&owner, Some(symbol));
        if !same_arguments(&owner, &instance.args) {
            chain.push(owner);
        }
    }
    chain.reverse();
    (chain, method)
}

/// Whether `body` calls the generic function instantiation `symbol`.
fn calls(body: &Body, symbol: &Symbol) -> bool {
    body.generic_function_calls
        .iter()
        .any(|call| call.symbol == *symbol)
}

/// Whether `body` names `class` at `args` other than through the instance it
/// runs at: a body names its own instance, and every instance nested inside
/// it, without having built any of them.
fn builds_instance(body: &Body, class: &str, args: &[Type]) -> bool {
    let names = body
        .generic_class_instantiations
        .iter()
        .any(|named| named.class == class && same_arguments(&named.type_args, args));
    names
        && !self_instance(body, class).is_some_and(|own| {
            same_arguments(&own, args) || own.iter().any(|arg| nests_instance(arg, class, args))
        })
}

/// Whether `ty` is, or holds somewhere inside it, `class` at exactly `args`.
fn nests_instance(ty: &Type, class: &str, args: &[Type]) -> bool {
    let (head, parts) = constructor_parts(ty);
    (head == class && same_arguments(&parts, args))
        || parts.iter().any(|part| nests_instance(part, class, args))
}

/// The arguments of the `class` instance `body` runs at, read off its first
/// parameter; `None` when it is not a method of `class`.
fn self_instance(body: &Body, class: &str) -> Option<Vec<Type>> {
    if body.arg_count == 0 {
        return None;
    }
    let ty = &body.local_decls.get(1)?.ty;
    let TypeKind::Custom(name, Some(arg_exprs)) = &ty.kind else {
        return None;
    };
    (name == class)
        .then(|| arg_exprs.iter().map(instantiation_argument).collect())
        .flatten()
}

/// The method of `class` whose body at `args` is emitted as `symbol`.
fn method_named(
    symbol: &Symbol,
    class: &str,
    args: &[Type],
    type_defs: &HashMap<String, TypeDefinition>,
) -> Option<String> {
    let Some(TypeDefinition::Class(definition)) = type_defs.get(class) else {
        return None;
    };
    symbol
        .method_of(class, args)
        .filter(|method| definition.methods.contains_key(*method))
        .map(str::to_string)
}

/// The type an argument of a generic-class reference stands for in a
/// diagnostic: its type or value, or the expression it still is.
fn spelled_argument(arg: &Expression) -> Type {
    instantiation_argument(arg)
        .unwrap_or_else(|| crate::type_checker::generics::value_generic_marker_type(arg.clone()))
}

/// Whether two instantiation tuples name one instantiation.
fn same_arguments(left: &[Type], right: &[Type]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a.kind == b.kind)
}

impl LoweringContext<'_> {
    /// Refuse an instance of a generic class at `ty`, built at `span`, whose
    /// arguments name no single instantiation.
    ///
    /// A type argument with no name to compile a body at — one nested past
    /// the depth types are named to, or a closure type declaring type
    /// parameters of its own — is refused wherever the instance is built:
    /// every such instance would otherwise share one compiled body and one
    /// drop function, each laid out for whichever instance claimed it first.
    /// An argument naming a parameter no substitution binds is not an
    /// instantiation yet and passes.
    ///
    /// A value argument is checked only in a body lowered for one
    /// instantiation. There every parameter is bound, so an argument computed
    /// from them (`Size * 2`) that does not fold would leave the instance
    /// running the body shared by every instantiation. An operand that is
    /// neither bound nor declared has no value at any instantiation and is
    /// refused.
    // TODO: a body shared by every instantiation of its declaration — a
    // method of `class Wrapper<T>` lowered once, or a generic function whose
    // parameter appears only in its return type — builds its instances at
    // open arguments and still runs them through shared bodies and the bare
    // vtable. Closing that needs those bodies specialized per instantiation.
    pub fn refuse_unnameable_instance(&self, ty: &Type, span: Span) -> Result<(), LoweringError> {
        let TypeKind::Custom(class, Some(arg_exprs)) = &ty.kind else {
            return Ok(());
        };
        let type_defs = self.type_checker.type_definitions();
        if type_defs
            .get(class)
            .and_then(TypeDefinition::generics)
            .is_none()
        {
            return Ok(());
        }
        let args: Vec<Type> = arg_exprs.iter().map(spelled_argument).collect();
        for (position, (arg, arg_ty)) in arg_exprs.iter().zip(&args).enumerate() {
            match super::type_argument(arg) {
                Some(written) if self.is_unnameable_type_argument(&written) => {
                    return Err(unnameable_type_argument(class, &args, arg_ty, span));
                }
                Some(_) => {}
                None if self.generic_subs.is_empty() => {}
                None => self.refuse_unfoldable_value(class, &args, (position, arg), span)?,
            }
        }
        Ok(())
    }

    /// Refuse `name` instantiated at `args`, reached at `span`, when one of
    /// the arguments has no name to compile a body at: every such
    /// instantiation would otherwise share one compiled body.
    pub fn refuse_unnameable_type_arguments(
        &self,
        name: &str,
        args: &[Type],
        span: Span,
    ) -> Result<(), LoweringError> {
        match args
            .iter()
            .find(|arg| self.is_unnameable_type_argument(arg))
        {
            Some(arg) => Err(unnameable_type_argument(name, args, arg, span)),
            None => Ok(()),
        }
    }

    /// Whether `arg` has no spelling to compile a body at and is not merely a
    /// type parameter still awaiting its substitution.
    fn is_unnameable_type_argument(&self, arg: &Type) -> bool {
        !super::has_a_monomorphized_spelling(&arg.kind)
            && !mentions_open_parameter(arg, self.type_checker.type_definitions())
    }

    /// Refuse the value argument `arg`, at `position` among the arguments of
    /// `class` at `args`, when it has no value at the substitution this body is
    /// lowered under.
    ///
    /// An argument that outgrows the integer range at the end of a chain of
    /// instances each built from the last says so: the chain is what to
    /// bound, not the one value.
    fn refuse_unfoldable_value(
        &self,
        class: &str,
        args: &[Type],
        (position, arg): (usize, &Expression),
        span: Span,
    ) -> Result<(), LoweringError> {
        let Some(cause) =
            unfoldable_value_argument(arg, &self.generic_subs, &self.body.type_params)
        else {
            return Ok(());
        };
        let bindings = bound_value_names(arg, &self.generic_subs);
        let error = invalid_value_argument(class, args, arg, &bindings, cause, span);
        let grows =
            cause == UnfoldableValue::OutOfRange && self.is_built_by_itself(class, arg, position);
        Err(match bindings.first() {
            Some((parameter, _)) if grows => error.with_note(value_growth_note(class, parameter)),
            Some(_) | None => error,
        })
    }

    /// Whether the registry holds an instance of `class` that builds the one
    /// this body runs at through `arg` at `position`: the body is then a step
    /// of a chain `arg` grows, not the first instance written.
    fn is_built_by_itself(&self, class: &str, arg: &Expression, position: usize) -> bool {
        let value_of =
            |ty: &Type| extract_value_generic(ty).and_then(TypeChecker::try_eval_const_int);
        let own = self_instance(&self.body, class);
        let Some(own_value) = own
            .as_ref()
            .and_then(|own| own.get(position))
            .and_then(value_of)
        else {
            return false;
        };
        let type_defs = self.type_checker.type_definitions();
        let Some(generics) = type_defs.get(class).and_then(TypeDefinition::generics) else {
            return false;
        };
        let registered = self.type_checker.generic_class_instantiations.get(class);
        registered.into_iter().flatten().any(|earlier| {
            let mapping: HashMap<String, Type> = generics
                .iter()
                .map(|generic| generic.name.clone())
                .zip(earlier.iter().cloned())
                .collect();
            fold_value_generic_arithmetic(arg, &mapping)
                .and_then(|folded| TypeChecker::try_eval_const_int(&folded))
                == Some(own_value)
        })
    }

    /// Record on the body each generic-class instantiation `ty` spells that the
    /// registry does not hold yet.
    ///
    /// Only a body lowered under an instantiation substitution records anything:
    /// everywhere else a type is what the checker inferred, and the registry was
    /// filled from exactly those. An instantiation with an argument that still
    /// has no concrete spelling names no body a call could reach, so it is left
    /// out.
    pub fn record_class_instantiations(&mut self, ty: &Type) {
        if self.generic_subs.is_empty() {
            return;
        }
        let found = self.unregistered_class_instantiations(std::iter::once(ty));
        self.push_class_instantiations(found);
    }

    /// Record the instantiations every local of the body is declared at.
    ///
    /// A binding, a parameter and a temporary are each dropped through the
    /// drop function of their own type, and a per-instantiation drop function
    /// exists only for a registered instantiation.
    pub fn record_local_class_instantiations(&mut self) {
        if self.generic_subs.is_empty() {
            return;
        }
        let found =
            self.unregistered_class_instantiations(self.body.local_decls.iter().map(|d| &d.ty));
        self.push_class_instantiations(found);
    }

    fn unregistered_class_instantiations<'t>(
        &self,
        types: impl Iterator<Item = &'t Type>,
    ) -> Vec<GenericClassInstantiation> {
        let mut found = Vec::new();
        for ty in types {
            collect_generic_instantiations(self.type_checker, &ty.kind, &mut found);
        }
        let definitions = self.type_checker.type_definitions();
        found
            .into_iter()
            .filter(|(class, type_args)| {
                type_args
                    .iter()
                    .all(|arg| super::is_monomorphizable_type_argument(&arg.kind, definitions))
                    && !is_registered_instantiation(self.type_checker, class, type_args)
            })
            .map(|(class, type_args)| GenericClassInstantiation { class, type_args })
            .collect()
    }

    fn push_class_instantiations(&mut self, found: Vec<GenericClassInstantiation>) {
        for instantiation in found {
            if !self
                .body
                .generic_class_instantiations
                .contains(&instantiation)
            {
                self.body.generic_class_instantiations.push(instantiation);
            }
        }
    }
}
