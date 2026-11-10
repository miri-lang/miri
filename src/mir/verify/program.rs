// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every verifier run over a whole lowered program, gathered into one report.

use std::collections::{HashMap, HashSet};

use super::VerificationViolation;
use crate::ast::BuiltinCollectionKind;
use crate::mir::facts::{ProgramFacts, StaleFact};
use crate::mir::symbol::Symbol;
use crate::type_checker::context::{ClassDefinition, TypeDefinition};

/// The type table the verifiers read declarations from.
type TypeDefinitions = HashMap<String, TypeDefinition>;

/// What the verifiers found, one line per violation, each naming the
/// function it was found in.
#[derive(Debug, Default)]
pub struct ProgramReport {
    pub lines: Vec<String>,
    /// How many functions the lines name.
    pub functions: usize,
}

impl ProgramReport {
    /// The report of `violations`, each naming the function it was found in.
    pub fn of(violations: Vec<(String, VerificationViolation)>) -> Self {
        let functions = (violations.iter().map(|(name, _)| name))
            .collect::<HashSet<_>>()
            .len();
        let lines = (violations.iter())
            .map(|(name, v)| format!("  fn {}: {}", name, v))
            .collect();
        Self { lines, functions }
    }

    /// Add a line for each vtable slot found pointing at a copy of a generic
    /// method; a slot belongs to no function, so the count of functions the
    /// report names is unchanged.
    pub fn add_slot_violations(&mut self, found: Vec<super::SlotViolation>) {
        self.lines.extend(
            found
                .into_iter()
                .map(|violation| format!("  vtable: {violation}")),
        );
    }

    /// The report as one message, or `None` when nothing was found.
    pub fn message(&self) -> Option<String> {
        (!self.lines.is_empty()).then(|| {
            format!(
                "{} MIR invariant violation(s) in {} function(s):\n{}",
                self.lines.len(),
                self.functions,
                self.lines.join("\n")
            )
        })
    }
}

/// The violations found across the program with every retain and release
/// in place: optional slots checked again, and every argument lent through
/// a class field to a call that may release it.
fn verify_program_after_rc(
    definitions: &TypeDefinitions,
    bodies: &[(String, crate::mir::Body)],
) -> Vec<(String, super::VerificationViolation)> {
    let hooks = crate::mir::type_facts::DropHookReach::new(definitions);
    let mut violations = super::verify_optional_slots(bodies, String::clone);
    violations.extend(super::verify_lent_arguments(bodies, String::clone, &hooks));
    violations
}

/// The calls of each of `bodies` to a function of the program the
/// verifiers' own rule finds harmless, over the program with every retain
/// and release in place.
fn harmless_calls_after_rc(
    definitions: &TypeDefinitions,
    bodies: &[(String, crate::mir::Body)],
) -> Vec<HashSet<crate::mir::BasicBlock>> {
    let hooks = crate::mir::type_facts::DropHookReach::new(definitions);
    let harmless = super::HarmlessCallees::of(bodies, String::clone, &hooks);
    bodies
        .iter()
        .map(|(_, body)| harmless.calls_in(body))
        .collect()
}

/// Run every verification pass over every body and gather what they find
/// into one report.
///
/// `program_violations` are the findings of checks run across bodies, each
/// already naming the function it was found in. `facts` are those of
/// `bodies`, in order; a body whose ownership facts no longer hold at its
/// generation is a fault of the compiler, reported as [`StaleFact`].
pub fn verify_program(
    definitions: &TypeDefinitions,
    bodies: &[(String, crate::mir::Body)],
    facts: &ProgramFacts,
    program_violations: Vec<(Symbol, super::VerificationViolation)>,
) -> Result<ProgramReport, StaleFact> {
    let shared_collection_methods = shared_collection_methods(definitions);
    let intrinsic_backed = collection_methods_backed_by_intrinsics(definitions);
    let mut functions_with_violations = program_violations
        .iter()
        .map(|(symbol, _)| symbol)
        .collect::<HashSet<_>>()
        .len();
    let mut all_violations: Vec<String> = program_violations
        .into_iter()
        .map(|(symbol, v)| format!("  fn {}: {}", symbol, v))
        .collect();
    let after_rc = verify_program_after_rc(definitions, bodies);
    functions_with_violations += after_rc
        .iter()
        .map(|(name, _)| name)
        .collect::<HashSet<_>>()
        .len();
    all_violations.extend(
        after_rc
            .into_iter()
            .map(|(name, v)| format!("  fn {}: {}", name, v)),
    );
    let harmless_calls = harmless_calls_after_rc(definitions, bodies);
    for (((name, body), harmless), facts) in bodies.iter().zip(&harmless_calls).zip(facts.iter()) {
        let ownership = facts.current_ownership(body)?;
        let mut violations =
            super::verify_body_with_harmless_calls(body, ownership, Some(harmless));
        violations.extend(super::verify_collection_element_ownership(
            body,
            &shared_collection_methods,
            &intrinsic_backed,
        ));
        violations.extend(super::verify_optional_representation(body, definitions));
        violations.extend(super::verify_value_representation(body, definitions));
        violations.extend(super::verify_value_arguments(body));
        violations.extend(super::verify_moved_out(body));
        // TODO: this check runs only under --verify-mir; a normal build does
        // not check that in-place writes land on unshared collections.
        violations.extend(super::verify_unshared_writes(body, ownership));
        if !violations.is_empty() {
            functions_with_violations += 1;
        }
        for v in violations {
            all_violations.push(format!("  fn {}: {}", name, v));
        }
    }
    Ok(ProgramReport {
        lines: all_violations,
        functions: functions_with_violations,
    })
}

/// Run every verifier that reads the bodies a backend receives over
/// `bodies`, and gather what they find into one report.
///
/// These checks hold of a body whatever spelling the element and closure ABI
/// gave its runtime calls, so they run after it: the accesses emitted without
/// a bounds check, the readback fences, the names a pattern binds and the
/// collections a loop leaves unchanged. Every other verifier reads the body as
/// lowered, in [`verify_program`]. `lowered` is each body before the ABI
/// passes ran, in the same order, when they did: each body is then checked to
/// differ from it only by the ABI's own expansions.
pub fn verify_backend_bodies(
    definitions: &TypeDefinitions,
    bodies: &[(String, crate::mir::Body)],
    facts: &ProgramFacts,
    lowered: Option<&[(String, crate::mir::Body)]>,
) -> Result<ProgramReport, StaleFact> {
    let harmless_calls = harmless_calls_after_rc(definitions, bodies);
    let mut found = Vec::new();
    for (index, (((name, body), harmless), facts)) in bodies
        .iter()
        .zip(&harmless_calls)
        .zip(facts.iter())
        .enumerate()
    {
        let ownership = facts.current_ownership(body)?;
        let mut violations = super::verify_in_bounds(body);
        violations.extend(super::verify_cross_residency_readback(body));
        violations.extend(super::verify_bound_names(body));
        violations.extend(super::verify_stable_collections(
            body,
            ownership,
            Some(harmless),
        ));
        if let Some((_, before)) = lowered.and_then(|lowered| lowered.get(index)) {
            violations.extend(super::verify_abi_translation(before, body));
        }
        found.extend(violations.into_iter().map(|v| (name.clone(), v)));
    }
    Ok(ProgramReport::of(found))
}

/// The shared body of every method a built-in collection is compiled with:
/// the symbols [`super::verify_collection_element_ownership`] holds to its
/// rule, spelled from the declarations rather than recognised by reading a
/// call's name.
fn shared_collection_methods(definitions: &TypeDefinitions) -> HashSet<Symbol> {
    builtin_collection_classes(definitions)
        .flat_map(|(class_name, _)| {
            crate::mir::lowering::dispatch_symbols::methods_compiled_under(definitions, class_name)
                .map(move |method_name| Symbol::method(class_name, &[], method_name, &[]))
        })
        .collect()
}

/// The shared collection method symbols whose bodies settle element
/// ownership by calling the runtime.
///
/// Those bodies do it through the runtime rather than by any RC operation MIR
/// can see, so the shared generic one is correct at every element type and
/// [`super::verify_collection_element_ownership`] exempts them. Every
/// other method needs the concrete element type instead —
/// [`crate::mir::lowering::method_dispatch::is_settled_by_the_runtime`] is the one
/// definition dispatch and this exemption share, and
/// [`crate::type_checker::runtime_settled`] states the rule.
fn collection_methods_backed_by_intrinsics(definitions: &TypeDefinitions) -> HashSet<Symbol> {
    builtin_collection_classes(definitions)
        .flat_map(|(class_name, class_def)| {
            class_def
                .methods
                .keys()
                .filter(|method_name| {
                    crate::mir::lowering::method_dispatch::is_settled_by_the_runtime(
                        class_def,
                        method_name,
                    )
                })
                .map(move |method_name| Symbol::method(class_name, &[], method_name, &[]))
        })
        .collect()
}

/// Every built-in collection the program's type table declares as a class.
fn builtin_collection_classes(
    definitions: &TypeDefinitions,
) -> impl Iterator<Item = (&str, &ClassDefinition)> {
    definitions
        .iter()
        .filter(|(class_name, _)| BuiltinCollectionKind::from_name(class_name).is_some())
        .filter_map(|(class_name, definition)| match definition {
            TypeDefinition::Class(class_def) => Some((class_name.as_str(), class_def)),
            TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_)
            | TypeDefinition::Trait(_) => None,
        })
}
