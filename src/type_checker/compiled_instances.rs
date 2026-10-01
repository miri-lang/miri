// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The check every compiled instance of a generic method passes, however the
//! lowering came to compile it.
//!
//! The requirement rail in [`super::instantiation_requirements`] answers a
//! body's obligations at each site the program uses the body at. Lowering
//! decides separately which instances to compile, from the bodies it lowers.
//! Wherever lowering compiles an instance no answered site stood for, the
//! body would run on a type nobody checked it against. So each compiled
//! instance is answered again here, by the same replay of the body's own
//! checks, and a refusal refuses the program instead of compiling the body.

use super::context::Context;
use super::instantiation_requirements::{Pin, PinningSite};
use super::TypeChecker;
use crate::ast::types::Type;
use crate::error::syntax::Span;
use crate::error::type_error::TypeError;
use std::collections::HashMap;

/// The scope a checked program's global declarations live in, kept after the
/// check so a compiled instance is answered in the scope every site was.
pub(crate) struct GlobalScope(pub(crate) Context);

impl std::fmt::Debug for GlobalScope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("GlobalScope")
    }
}

/// A method compiled at one instance of its class.
#[derive(Debug, Clone)]
pub struct CompiledInstance {
    pub class_name: String,
    pub method: String,
    /// What the instance pins the class's parameters to; empty for a class
    /// with none, whose clauses pin what the method reads.
    pub substitution: HashMap<String, Type>,
    /// The instance's own type, which a trait default reads `self` at.
    pub receiver: Type,
    /// Where the compiled method is declared.
    pub span: Span,
}

impl TypeChecker {
    /// The refusals compiling `instance` earns: each obligation stated by a
    /// body its method runs — the class's own, or one a type above it
    /// declares — answered at the instance, together with every method those
    /// bodies are known to run on what they pin. Empty when every obligation holds
    /// there, which it does wherever the program uses the method at that
    /// instance.
    ///
    /// Answering leaves the checker's own diagnostics as they were.
    pub fn compiled_instance_refusals(&mut self, instance: &CompiledInstance) -> Vec<TypeError> {
        let CompiledInstance {
            class_name,
            method,
            substitution,
            receiver,
            span,
        } = instance;
        let span = *span;
        let Some(scope) = self.global_scope.take() else {
            return Vec::new();
        };
        let requirements = std::mem::take(&mut self.instantiation_requirements);
        let errors_before = self.diagnostics.errors.len();
        let reported_before = self.diagnostics.reported_errors.clone();
        let mut sites: Vec<PinningSite> = self
            .bodies_run_for(class_name, method, substitution, receiver)
            .into_iter()
            .map(|(callee, pins)| PinningSite {
                caller: None,
                callee,
                pins: pins
                    .into_iter()
                    .map(|(parameter, pinned)| (parameter, Pin::Concrete(pinned)))
                    .collect(),
                span,
                caller_parameters: Vec::new(),
                reached_because: None,
            })
            .collect();
        // The methods these bodies run on what they pin — an element's
        // `compare`, a method called on a bounded parameter — are answered
        // too, exactly as the checker answers a site the program writes.
        let mut derivation = super::used_methods::SiteDerivation::rooted_at(sites.len());
        while self.derive_used_method_sites(&requirements, &mut sites, &mut derivation) {}
        for site in &sites {
            let stated = requirements
                .get(&site.callee)
                .map_or(&[][..], Vec::as_slice);
            self.answer_pinning_site(stated, site, &scope.0);
        }
        self.instantiation_requirements = requirements;
        self.global_scope = Some(scope);
        self.diagnostics.reported_errors = reported_before;
        self.diagnostics.errors.split_off(errors_before)
    }
}
