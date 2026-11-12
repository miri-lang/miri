// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The rule that ownership was decided over every instance lowering compiles.
//!
//! The type checker decides which types are resources, which values a body
//! may keep and where each hook runs over one set of generic class
//! instantiations. Lowering finds the instances it compiles bodies for on a
//! walk of its own: the vtables reached bodies build, the class methods
//! compiled per instance, and the generic copies whose type arguments name a
//! class instance. An instance lowering compiles that runs a drop hook, holds
//! one, or takes part in virtual dispatch, and that the decided set lacks,
//! means ownership was decided over too small a set: a refusal the program
//! earns may be missed, and a hook may run later than its owner's end.
//!
//! The rule reads only the decided set, the instances lowering reports and
//! the type table. It identifies an instance by [`TypeInstance`], the one
//! identity symbols are mangled from, so two spellings of one instantiation
//! agree.
//!
//! Its findings are warnings: they are reported beside the program's other
//! findings, never fail a build.

use std::collections::{HashMap, HashSet};
use std::fmt;

use crate::ast::type_arguments::type_arguments;
use crate::ast::types::{Type, TypeKind};
use crate::mir::symbol::{Symbol, TypeInstance};
use crate::type_checker::context::{class_needs_vtable, TypeDefinition};
use crate::type_checker::utils::{Instantiations, ResourceTypes};

/// A generic class instance lowering compiled a body for, at the type
/// arguments it was compiled at.
#[derive(Debug, Clone, PartialEq)]
pub struct ReachedInstance {
    pub class: String,
    pub args: Vec<Type>,
    pub origin: ReachedOrigin,
}

/// What made lowering compile a body for an instance.
#[derive(Debug, Clone, PartialEq)]
pub enum ReachedOrigin {
    /// A reached body builds the instance, so its vtable is written.
    Vtable,
    /// The named method of the class is compiled at the instance.
    Method(String),
    /// The generic copy compiled under the symbol names the instance among
    /// its type arguments or as its owner.
    Copy(Symbol),
}

impl fmt::Display for ReachedOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vtable => write!(f, "its vtable"),
            Self::Method(method) => write!(f, "its method `{method}`"),
            Self::Copy(symbol) => write!(f, "the generic copy `{symbol}`"),
        }
    }
}

/// An instance lowering compiled a body for that ownership was not decided
/// over.
#[derive(Debug, Clone, PartialEq)]
pub struct AgreementGap {
    pub class: String,
    pub args: Vec<Type>,
    /// The first reason lowering compiled a body for the instance.
    pub origin: ReachedOrigin,
    /// Whether a value of the instance runs a drop hook or holds one; an
    /// instance that is no resource is reported for taking part in virtual
    /// dispatch.
    pub is_resource: bool,
}

impl fmt::Display for AgreementGap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let args: Vec<String> = self.args.iter().map(|arg| arg.kind.to_string()).collect();
        let bearing = match self.is_resource {
            true => "a resource",
            false => "dispatched through",
        };
        write!(
            f,
            "`{}<{}>` ({bearing}) is compiled for {} but is not among the instantiations \
             ownership was decided over",
            self.class,
            args.join(", "),
            self.origin
        )
    }
}

/// Each instance among `reached` that runs a drop hook, holds one or takes
/// part in virtual dispatch, so that a trait or a base class is a resource
/// over it, and that `decided`, the instantiations ownership
/// was decided over, does not list; one finding per instance, naming the
/// first reason lowering compiled it.
pub fn verify_reached_agreement(
    definitions: &HashMap<String, TypeDefinition>,
    decided: &Instantiations,
    reached: &[ReachedInstance],
) -> Vec<AgreementGap> {
    let listed: HashSet<TypeInstance> = (decided.iter())
        .flat_map(|(class, tuples)| tuples.iter().map(|args| TypeInstance::new(class, args)))
        .collect();
    let resources = ResourceTypes::of(definitions, decided);
    let mut reported = HashSet::new();
    reached
        .iter()
        .filter(|instance| !instance.args.is_empty())
        .filter(|instance| {
            let identity = TypeInstance::new(&instance.class, &instance.args);
            !listed.contains(&identity) && reported.insert(identity)
        })
        .filter_map(|instance| {
            let is_resource = is_resource(instance, &resources);
            let bears_on_ownership =
                is_resource || class_needs_vtable(&instance.class, definitions);
            bears_on_ownership.then(|| AgreementGap {
                class: instance.class.clone(),
                args: instance.args.clone(),
                origin: instance.origin.clone(),
                is_resource,
            })
        })
        .collect()
}

/// Whether a value of `instance` runs a drop hook or holds one.
fn is_resource(instance: &ReachedInstance, resources: &ResourceTypes) -> bool {
    let arguments = type_arguments(instance.args.iter().cloned());
    resources.holds(&TypeKind::Custom(instance.class.clone(), Some(arguments)))
}
