// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The bounds the walk over reached bodies applies, and the refusal of the
//! first body or built class past one, worded as lowering words it
//! ([`crate::type_checker::instantiation_limits`]).

use super::{Lineage, ReachWalk, Step};
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::RelatedNote;
use crate::error::syntax::Span;
use crate::error::type_error::TypeError;
use crate::type_checker::context::{Context, TypeDefinition};
use crate::type_checker::function_analysis::ModuleId;
use crate::type_checker::instantiation_limits::{
    arguments_type_size, constructor_parts, deepest_argument_depth, exceeded_limit, growth_refusal,
    has_value_argument, Constructor, ExceededLimit, Growth, MAX_INSTANCE_TYPE_DEPTH,
    MAX_INSTANCE_TYPE_SIZE,
};
use crate::type_checker::instantiation_requirements::{free_function_owner, GenericBodyId};
use crate::type_checker::TypeChecker;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// A top-level generic function as the program's global scope declares it.
struct DeclaredFunction<'c> {
    /// Its type parameters, in order.
    parameters: Vec<&'c str>,
    /// The type parameter its first parameter is written as, if any.
    first: Option<&'c str>,
}

/// The refusal of the first instance found past a bound.
pub(super) type Refused = Box<TypeError>;

/// The instances of a class that led to an instance past a bound, outermost
/// first, the method building the next one, and whether they were reached
/// through a trait.
pub(super) type GrowthChain = (Vec<Vec<Type>>, Option<String>, bool);

impl TypeChecker {
    /// Refuse the body `step` reaches, from a site at `span`, where it runs
    /// at an instance of its class past a bound, or its own type parameters
    /// are bound past the bounds on a generic function's.
    pub(super) fn refuse_body_past_limits(
        &self,
        step: &Rc<Step>,
        span: Span,
        context: &Context,
        walk: &mut ReachWalk<'_>,
    ) -> Result<(), Refused> {
        let (owner, method) = &step.body;
        let Some(TypeDefinition::Class(class)) = self.type_definitions().get(owner) else {
            return self.refuse_function_past_limits(step, span, context, walk);
        };
        let declared = (class.generics.iter().flatten()).map(|generic| generic.name.as_str());
        let arguments: Vec<Type> = bound_types(declared, &step.bound).cloned().collect();
        if !arguments.is_empty() {
            let growth = || {
                if step.through_slot {
                    self.growth_through_slots(owner, &step.outer, context)
                } else {
                    self.growth_through_calls(owner, &arguments, &step.outer, context)
                }
            };
            self.refuse_instance_past_limits(owner, &arguments, growth, (span, step), walk)?;
        }
        let own = (class.methods.get(method).into_iter())
            .flat_map(|info| info.generics.iter().map(|generic| generic.name.as_str()));
        let own: Vec<&Type> = bound_types(own, &step.bound).collect();
        refuse_call_past_limits(method, &own, (span, step))
    }

    /// Refuse the generic function `step` reaches, from a site at `span`,
    /// where it is bound past the bounds on a generic function's. An
    /// argument that is itself an instance of a generic type past a bound is
    /// refused as that instance, the way lowering refuses the instance the
    /// body building it builds before the function it is handed to.
    fn refuse_function_past_limits(
        &self,
        step: &Rc<Step>,
        span: Span,
        context: &Context,
        walk: &mut ReachWalk<'_>,
    ) -> Result<(), Refused> {
        let method = &step.body.1;
        let Some(function) = self.declared_function(&step.body, context) else {
            return refuse_call_past_limits(method, &types_by_name(&step.bound), (span, step));
        };
        let arguments: Vec<&Type> =
            bound_types(function.parameters.iter().copied(), &step.bound).collect();
        if deepest_argument_depth(arguments.iter().copied()) > MAX_INSTANCE_TYPE_DEPTH {
            for argument in &arguments {
                let (Constructor::Named(class), parts) = constructor_parts(argument) else {
                    continue;
                };
                if !self.type_definitions().contains_key(class) {
                    continue;
                }
                let growth = || self.growth_through_calls(class, &parts, &step.outer, context);
                self.refuse_instance_past_limits(class, &parts, growth, (span, step), walk)?;
            }
        }
        refuse_call_past_limits(method, &arguments, (span, step))
    }

    /// The top-level generic function `body` names, as the program's global
    /// scope `context` declares it; `None` for a body no top-level function
    /// declares.
    fn declared_function<'c>(
        &self,
        (owner, name): &GenericBodyId,
        context: &'c Context,
    ) -> Option<DeclaredFunction<'c>> {
        let info = context.resolve_info(name)?;
        if free_function_owner(&ModuleId::checked_as(&info.module)) != *owner {
            return None;
        }
        let TypeKind::Function(function) = &info.ty.kind else {
            return None;
        };
        let parameters: Vec<&str> = (function.generics.iter().flatten())
            .filter_map(declared_parameter_name)
            .collect();
        let first = (function.params.first())
            .and_then(|parameter| written_parameter_name(&parameter.typ))
            .filter(|written| parameters.contains(written));
        Some(DeclaredFunction { parameters, first })
    }

    /// Refuse the instance of `class` at `arguments`, built at `span` in or
    /// for the body `within` reached, where it passes a bound of
    /// [`crate::type_checker::instantiation_limits`] and a recursion reached
    /// it ([`is_recursive`]), explained by the chain `growth` answers.
    pub(super) fn refuse_instance_past_limits(
        &self,
        class: &str,
        arguments: &[Type],
        growth: impl FnOnce() -> GrowthChain,
        (span, within): (Span, &Step),
        walk: &mut ReachWalk<'_>,
    ) -> Result<(), Refused> {
        let value_instances = if has_value_argument(arguments) {
            let found = walk.value_instances.entry(class.to_string()).or_default();
            found.insert(arguments.iter().map(|ty| ty.kind.clone()).collect());
            found.len()
        } else {
            0
        };
        let type_defs = self.type_definitions();
        let Some(limit) = exceeded_limit(class, arguments, value_instances, type_defs) else {
            return Ok(());
        };
        if !is_recursive(within) {
            return Ok(());
        }
        let (chain, method, through_trait) = growth();
        let growth = Growth {
            chain: chain.iter().map(Vec::as_slice).collect(),
            method: method.as_deref(),
            through_trait,
        };
        Err(polymorphic_recursion(
            class, arguments, &limit, &growth, span,
        ))
    }

    /// The instances of `class` whose slots the calls through a trait on
    /// `lineage` read, outermost first, and the method of the last of them,
    /// as lowering names the chain of an instance it builds for a vtable.
    pub(super) fn growth_through_slots(
        &self,
        class: &str,
        lineage: &Lineage,
        context: &Context,
    ) -> GrowthChain {
        let mut chain = Vec::new();
        let mut method = None;
        for step in steps(lineage).filter(|step| step.through_slot && step.body.0 == class) {
            let Some(arguments) = self.running_instance(class, step, context) else {
                continue;
            };
            method.get_or_insert_with(|| step.body.1.clone());
            chain.push(arguments);
        }
        chain.reverse();
        (chain, method, true)
    }

    /// The instances of `class` whose methods called each other on the way
    /// to the instance at `arguments`, outermost first, and the method of the
    /// last of them: the bodies on `lineage` running at an instance of
    /// `class`, back to the first that does not, as lowering names the chain
    /// of an instance a static call reaches. A method of `class` runs at the
    /// instance it is reached at, and a generic function at the instance its
    /// first parameter is bound to.
    pub(super) fn growth_through_calls(
        &self,
        class: &str,
        arguments: &[Type],
        lineage: &Lineage,
        context: &Context,
    ) -> GrowthChain {
        let mut chain: Vec<Vec<Type>> = Vec::new();
        let mut method = None;
        for step in steps(lineage) {
            let Some(instance) = self.running_instance(class, step, context) else {
                break;
            };
            if step.body.0 == class {
                method.get_or_insert_with(|| step.body.1.clone());
            }
            let previous = (chain.last().map(Vec::as_slice)).unwrap_or(arguments);
            if !same_arguments(previous, &instance) && !same_arguments(arguments, &instance) {
                chain.push(instance);
            }
        }
        chain.reverse();
        (chain, method, false)
    }

    /// The arguments of the instance of `class` the body `step` reached runs
    /// at, in the order `class` declares its parameters: the instance a
    /// method of `class` is reached at, or the one a generic function's first
    /// parameter is bound to.
    fn running_instance(&self, class: &str, step: &Step, context: &Context) -> Option<Vec<Type>> {
        if step.body.0 != class {
            let first = self.declared_function(&step.body, context)?.first?;
            let (Constructor::Named(name), parts) = constructor_parts(step.bound.get(first)?)
            else {
                return None;
            };
            return (name == class).then_some(parts);
        }
        let Some(TypeDefinition::Class(definition)) = self.type_definitions().get(class) else {
            return None;
        };
        (definition.generics.iter().flatten())
            .map(|generic| step.bound.get(&generic.name).cloned())
            .collect()
    }
}

/// The steps of `lineage`, innermost first.
fn steps(lineage: &Lineage) -> impl Iterator<Item = &Step> {
    std::iter::successors(lineage.as_deref(), |step| step.outer.as_deref())
}

/// Whether a body on the way to `step`, `step`'s own included, was reached
/// from itself: only a body reached through a recursion can reach types
/// that grow without end. A type the program writes past a bound, reached
/// by no recursion, is left to lowering, which names it for what it is.
fn is_recursive(step: &Step) -> bool {
    let mut seen = HashSet::new();
    std::iter::successors(Some(step), |step| step.outer.as_deref())
        .any(|step| !seen.insert(&step.body))
}

/// The types `bound` gives each of `parameters` it binds, in order.
fn bound_types<'b>(
    parameters: impl Iterator<Item = &'b str>,
    bound: &'b HashMap<String, Type>,
) -> impl Iterator<Item = &'b Type> {
    parameters.filter_map(|name| bound.get(name))
}

/// The types `bound` gives its parameters, in the order of their names.
fn types_by_name(bound: &HashMap<String, Type>) -> Vec<&Type> {
    let mut named: Vec<(&String, &Type)> = bound.iter().collect();
    named.sort_by_key(|(name, _)| *name);
    named.into_iter().map(|(_, ty)| ty).collect()
}

/// Refuse the generic function or method `name` bound at `arguments`, at the
/// site at `span` reaching it as the body `within` reached, where a recursion
/// reached it ([`is_recursive`]) and they nest past [`MAX_INSTANCE_TYPE_DEPTH`]
/// or are built from more than [`MAX_INSTANCE_TYPE_SIZE`] types, as lowering
/// refuses the instantiation it would compile.
fn refuse_call_past_limits(
    name: &str,
    arguments: &[&Type],
    (span, within): (Span, &Step),
) -> Result<(), Refused> {
    let depth = deepest_argument_depth(arguments.iter().copied());
    let limit = if depth > MAX_INSTANCE_TYPE_DEPTH {
        ExceededLimit::TypeDepth(depth)
    } else if arguments_type_size(arguments.iter().copied()) > MAX_INSTANCE_TYPE_SIZE {
        ExceededLimit::TypeSize
    } else {
        return Ok(());
    };
    if !is_recursive(within) {
        return Ok(());
    }
    let arguments: Vec<Type> = arguments.iter().map(|&ty| ty.clone()).collect();
    Err(polymorphic_recursion(
        name,
        &arguments,
        &limit,
        &Growth::default(),
        span,
    ))
}

/// The refusal of the instance of `class` at `arguments`, built at `span`,
/// that passes `limit`, worded as lowering words it.
fn polymorphic_recursion(
    class: &str,
    arguments: &[Type],
    limit: &ExceededLimit,
    growth: &Growth,
    span: Span,
) -> Refused {
    let refusal = growth_refusal(class, arguments, limit, growth);
    let mut error = TypeError::coded(
        DiagnosticCode::MirPolymorphicRecursion,
        refusal.message,
        span,
        Some(refusal.help.to_string()),
    );
    error.related = (refusal.note.into_iter())
        .map(|message| RelatedNote {
            message,
            span: None,
        })
        .collect();
    Box::new(error)
}

/// Whether two argument lists name one instance.
fn same_arguments(left: &[Type], right: &[Type]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a.kind == b.kind)
}

/// The name a type parameter's declaration gives it.
fn declared_parameter_name(generic: &Expression) -> Option<&str> {
    let ExpressionKind::GenericType(name, _, _) = &generic.node else {
        return None;
    };
    let ExpressionKind::Identifier(name, _) = &name.node else {
        return None;
    };
    Some(name)
}

/// The name a parameter's written type spells, when it is a bare name.
fn written_parameter_name(written: &Expression) -> Option<&str> {
    let ExpressionKind::Type(ty, false) = &written.node else {
        return None;
    };
    let (TypeKind::Custom(name, None) | TypeKind::Generic(name, _)) = &ty.kind else {
        return None;
    };
    Some(name)
}
