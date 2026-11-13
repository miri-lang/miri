// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The base instances a class instance reaches through its `extends`
//! clause. An instance of `Child<String>` is an instance of every class
//! above it, at the arguments each `extends` clause restates from the one
//! below, and lowering compiles the bodies it inherits there. So each
//! instance the program builds, the classes declaring no parameters of their
//! own, and each class a reached body builds add the instances above them.

use super::limits::Refused;
use super::{is_open_kind, ReachWalk, Step};
use crate::ast::types::Type;
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, TypeDefinition};
use crate::type_checker::instantiation_requirements::spells_a_type;
use crate::type_checker::utils::add_instantiation;
use crate::type_checker::TypeChecker;
use std::rc::Rc;

impl TypeChecker {
    /// Record the instances above each class instance the program builds
    /// outside every generic body, and above each class declaring no
    /// parameters, in [`TypeChecker::reached_class_instantiations`]. No
    /// recursion leads to one of them, so none is refused.
    pub(super) fn record_rooted_bases(&mut self) {
        let built = (self.generic_class_instantiations.iter()).flat_map(|(class, tuples)| {
            tuples
                .iter()
                .map(move |arguments| (class.as_str(), arguments.as_slice()))
        });
        let plain = (self.type_definitions().iter())
            .filter(|(_, definition)| {
                matches!(definition, TypeDefinition::Class(class)
                    if class.generics.as_ref().is_none_or(Vec::is_empty))
            })
            .map(|(name, _)| (name.as_str(), &[][..]));
        let bases: Vec<(String, Vec<Type>)> = (built.chain(plain))
            .flat_map(|(class, arguments)| self.bases_above(class, arguments))
            .collect();
        for (base, arguments) in bases {
            self.record_reached_base(&base, arguments);
        }
    }

    /// Record the instances above `class` at `arguments`, which the body
    /// `step` reached builds at `span`, refusing one past a bound of
    /// [`crate::type_checker::instantiation_limits`] that a recursion leads
    /// to, as a class that body builds is refused.
    pub(super) fn record_bases_built_at(
        &mut self,
        (class, arguments): (&str, &[Type]),
        (span, step): (Span, &Rc<Step>),
        context: &Context,
        walk: &mut ReachWalk<'_>,
    ) -> Result<(), Refused> {
        let builder = Some(Rc::clone(step));
        for (base, base_arguments) in self.bases_above(class, arguments) {
            let growth = || self.growth_of(&base, &base_arguments, &builder, context);
            let within = (span, step.as_ref());
            self.refuse_instance_past_limits(&base, &base_arguments, growth, within, walk)?;
            self.record_reached_base(&base, base_arguments);
        }
        Ok(())
    }

    /// Record `class` at `arguments`, which no recursion leads to, with the
    /// instances above it.
    pub(super) fn record_with_bases(&mut self, class: &str, arguments: Vec<Type>) {
        let above = self.bases_above(class, &arguments);
        self.record_reached_base(class, arguments);
        for (base, base_arguments) in above {
            self.record_reached_base(&base, base_arguments);
        }
    }

    /// Record the instance `base` at `arguments`.
    fn record_reached_base(&mut self, base: &str, arguments: Vec<Type>) {
        if let Some(arguments) = self.instantiation_arguments(&arguments) {
            add_instantiation(&mut self.reached_class_instantiations, base, arguments);
        }
    }

    /// Each generic class above `class` at `arguments`, nearest first, at the
    /// arguments the `extends` clauses on the way up restate, as long as
    /// they leave no parameter open. A circular `extends`, reported where the
    /// class is declared, ends the walk after one step per definition.
    fn bases_above(&self, class: &str, arguments: &[Type]) -> Vec<(String, Vec<Type>)> {
        let mut above: Vec<(String, Vec<Type>)> = Vec::new();
        let mut next = self.base_instance(class, arguments);
        while let Some(base) = next {
            if above.len() >= self.type_definitions().len() {
                break;
            }
            next = self.base_instance(&base.0, &base.1);
            above.push(base);
        }
        above
    }

    /// The generic class `class` extends, at the arguments its `extends`
    /// clause restates at `arguments`, read through
    /// [`TypeChecker::ancestor_substitution`]. `None` when it extends
    /// nothing, extends a class declaring no parameters, or the clause
    /// leaves a parameter open there.
    fn base_instance(&self, class: &str, arguments: &[Type]) -> Option<(String, Vec<Type>)> {
        let Some(TypeDefinition::Class(definition)) = self.type_definitions().get(class) else {
            return None;
        };
        let base = definition.base_class.as_deref()?;
        let Some(TypeDefinition::Class(base_definition)) = self.type_definitions().get(base) else {
            return None;
        };
        let parameters = base_definition.generics.as_deref().unwrap_or_default();
        if parameters.is_empty() {
            return None;
        }
        let substitution = self.ancestor_substitution(class, arguments, base)?;
        let base_arguments: Vec<Type> = (parameters.iter())
            .map(|parameter| substitution.get(&parameter.name).cloned())
            .collect::<Option<_>>()?;
        let is_open = (base_arguments.iter()).any(|ty| spells_a_type(&ty.kind, &is_open_kind));
        (!is_open).then(|| (base.to_string(), base_arguments))
    }
}
