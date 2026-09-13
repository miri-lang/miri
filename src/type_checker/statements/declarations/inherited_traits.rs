// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What a class takes over from the traits the classes it extends implement.
//!
//! A trait an ancestor implements is implemented by every class below it, and a
//! trait method one of those ancestors declares answers for the subclass too —
//! with one exception. A method whose declared type names the trait's `Self` in
//! its return builds a value of the class that declared it, so the body a
//! subclass would inherit hands back an ancestor where the trait promises the
//! subclass. Every class that extends such a declaration therefore declares the
//! method itself: with a body, or abstract to leave it to its own descendants.

use crate::ast::expression::Expression;
use crate::ast::types::{Type, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::type_checker::context::{
    class_ancestry, class_is_or_extends, class_method_declaration, MethodInfo, TypeDefinition,
};
use crate::type_checker::TypeChecker;
use std::collections::{BTreeMap, BTreeSet, HashMap};

impl TypeChecker {
    /// Refuse a subclass that would inherit a body for a trait method whose
    /// return names `Self`, from any trait it or a class it extends implements.
    pub(crate) fn check_inherited_self_returns(
        &mut self,
        name: &str,
        base_name: &str,
        trait_names: &[String],
        methods: &BTreeMap<String, MethodInfo>,
        name_expr: &Expression,
    ) {
        let mut refusals: Vec<String> = Vec::new();
        let mut reported: BTreeSet<String> = BTreeSet::new();
        for (trait_name, method_name, method) in self.trait_methods_reaching(base_name, trait_names)
        {
            if methods.contains_key(&method_name)
                || !self.return_names_trait_self(&method, &trait_name)
            {
                continue;
            }
            let Some((declaring, _)) = class_method_declaration(
                base_name,
                &method_name,
                &self.type_table.global_type_definitions,
            ) else {
                continue;
            };
            if reported.insert(method_name.clone()) {
                refusals.push(format!(
                    "Class '{name}' must declare its own '{method_name}': the body it inherits \
                     from '{declaring}' builds a '{declaring}', and '{trait_name}' requires \
                     '{method_name}' to return 'Self'"
                ));
            }
        }
        for message in refusals {
            self.report_error(DiagnosticCode::TypTraitDefinition, message, name_expr.span);
        }
    }

    /// Every method of every trait the class names or a class it extends
    /// names, with the parent traits of each, tagged with the trait declaring it.
    fn trait_methods_reaching(
        &self,
        base_name: &str,
        trait_names: &[String],
    ) -> Vec<(String, String, MethodInfo)> {
        let definitions = &self.type_table.global_type_definitions;
        let mut pending: Vec<String> = trait_names.to_vec();
        for (_, ancestor) in class_ancestry(base_name, definitions) {
            pending.extend(ancestor.traits.iter().cloned());
        }
        pending.reverse();

        let mut visited: BTreeSet<String> = BTreeSet::new();
        let mut found = Vec::new();
        while let Some(trait_name) = pending.pop() {
            if !visited.insert(trait_name.clone()) {
                continue;
            }
            let Some(TypeDefinition::Trait(trait_def)) = definitions.get(&trait_name) else {
                continue;
            };
            for (method_name, method) in &trait_def.methods {
                found.push((trait_name.clone(), method_name.clone(), method.clone()));
            }
            pending.extend(trait_def.parent_traits.iter().rev().cloned());
        }
        found
    }

    /// True when `method`'s return type names `Self` of the trait that declares
    /// it, at any depth (`Self`, `Self?`, `List<Self>`).
    ///
    /// Inside a trait `Self` resolves to the trait's own type, so replacing that
    /// name and seeing the type change is what finds it.
    pub(crate) fn return_names_trait_self(&self, method: &MethodInfo, trait_name: &str) -> bool {
        let marker = Type::new(TypeKind::Error, method.return_type.span);
        let mapping = HashMap::from([(trait_name.to_string(), marker)]);
        self.substitute_type(&method.return_type, &mapping).kind != method.return_type.kind
    }

    /// True when class `candidate` is class `ancestor` or extends it, where
    /// `candidate` may be the class being declared — `declared`, extending
    /// `declared_base` — whose own definition is registered only once its checks
    /// have run.
    pub(crate) fn declared_class_is_or_extends(
        &self,
        candidate: &str,
        ancestor: &str,
        declared: &str,
        declared_base: Option<&str>,
    ) -> bool {
        let definitions = &self.type_table.global_type_definitions;
        if candidate != declared {
            return class_is_or_extends(candidate, ancestor, definitions);
        }
        candidate == ancestor
            || declared_base.is_some_and(|base| class_is_or_extends(base, ancestor, definitions))
    }
}
