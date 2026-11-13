// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The bodies lowering compiles whether or not a call reaches them. A class
//! declaring no parameters is compiled with every method it runs, its own
//! and each trait default its `implements` clauses, or those of a class it
//! extends, supply. Its own methods are no generic body, so every site they
//! write already roots the walk; a default is a generic body, read with the
//! trait's parameters at what the clauses pin them to and `Self` at the
//! class. So each such default roots the walk at those types, where its
//! obligations hold there, as lowering withholds a copy where they fail.

use crate::ast::types::{Type, TypeKind};
use crate::error::syntax::Span;
use crate::type_checker::compiled_instances::CompiledInstance;
use crate::type_checker::context::{
    class_ancestry, resolve_method_source, Context, MethodSource, TypeDefinition,
};
use crate::type_checker::instantiation_requirements::PinningSite;
use crate::type_checker::vtable_selectors::trait_hierarchy;
use crate::type_checker::TypeChecker;
use std::collections::{BTreeSet, HashMap};

impl TypeChecker {
    /// A site for each trait default a concrete class declaring no
    /// parameters runs, pinning the default at the class, in class order.
    /// `context` is the program's global scope.
    pub(super) fn default_copy_sites(&mut self, context: &Context) -> Vec<PinningSite> {
        let mut sites = Vec::new();
        for class in self.classes_without_parameters() {
            for (method, trait_name) in self.inherited_defaults(&class) {
                if !self.compiles_default_copy(&class, &method, context) {
                    continue;
                }
                sites.extend(self.default_copy_site(&class, (&trait_name, &method), context));
            }
        }
        sites
    }

    /// Every concrete class declaring no parameters of its own, by name,
    /// sorted.
    fn classes_without_parameters(&self) -> Vec<String> {
        let mut classes: Vec<String> = (self.type_definitions().iter())
            .filter(|(_, definition)| {
                matches!(definition, TypeDefinition::Class(class)
                    if !class.is_abstract && class.generics.as_ref().is_none_or(Vec::is_empty))
            })
            .map(|(name, _)| name.clone())
            .collect();
        classes.sort();
        classes
    }

    /// Each method `class` runs a trait default for, with the trait
    /// supplying it, sorted by method.
    fn inherited_defaults(&self, class: &str) -> Vec<(String, String)> {
        let definitions = self.type_definitions();
        let traits = class_ancestry(class, definitions)
            .flat_map(|(_, definition)| definition.traits.iter().map(String::as_str));
        let defaulted: BTreeSet<&str> = (trait_hierarchy(definitions, traits))
            .flat_map(|(_, definition)| definition.methods.iter())
            .filter(|(_, method)| !method.is_abstract)
            .map(|(name, _)| name.as_str())
            .collect();
        (defaulted.into_iter())
            .filter_map(
                |method| match resolve_method_source(definitions, class, method)? {
                    MethodSource::Default(default) => {
                        Some((method.to_string(), default.trait_name.to_string()))
                    }
                    MethodSource::Declared { .. } | MethodSource::AbstractOnly { .. } => None,
                },
            )
            .collect()
    }

    /// Whether lowering compiles `class`'s copy of the default `method`: its
    /// obligations hold at the class.
    fn compiles_default_copy(&mut self, class: &str, method: &str, context: &Context) -> bool {
        let instance = CompiledInstance {
            class_name: class.to_string(),
            method: method.to_string(),
            substitution: HashMap::new(),
            receiver: class_type(class),
            span: Span::default(),
        };
        self.instance_refusals_in(&instance, context).is_empty()
    }

    /// The site pinning `trait_name`'s default `method` at what `class`'s
    /// clauses pin the trait's parameters to, with `Self` at the class.
    fn default_copy_site(
        &self,
        class: &str,
        (trait_name, method): (&str, &str),
        context: &Context,
    ) -> Option<PinningSite> {
        let receiver = class_type(class);
        let (body, pins) = (self.bodies_run_for(class, method, &HashMap::new(), &receiver))
            .into_iter()
            .find(|((declaring, _), _)| declaring == trait_name)?;
        self.pinning_site(body, &pins, Span::default(), context)
    }
}

/// The type naming `class`, which declares no parameters.
fn class_type(class: &str) -> Type {
    Type::new(TypeKind::Custom(class.to_string(), None), Span::default())
}
