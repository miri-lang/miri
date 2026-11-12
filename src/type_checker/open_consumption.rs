// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Consuming a value of an open type, decided per copy of the body.
//!
//! A trait's default is compiled once for each class inheriting it, and a
//! method of a generic class once for each instantiation. Inside the one body
//! the type checker reads, a default's `self` is `Self` and a value of the
//! class's type parameter `T` is a `T`: neither is a resource there, yet a
//! copy may read it at one. Whether keeping such a value consumes it is
//! therefore decided per copy, from one record the consumption check
//! ([`super::use_after_move`]) makes once on the shared body:
//!
//! - each site that would consume the value were it a resource, a store
//!   ([`super::use_after_move::ConsumedArguments::consumes_if_handed_on`])
//!   or a kept closure's capture
//!   ([`super::use_after_move::ConsumedArguments::captured_by`]), which MIR
//!   lowering moves at a copy whose type hands the reference on
//!   ([`hands_on_at_copy`]) and copies, funded, at any other;
//! - each use after such a site ([`OpenRefusal`]), refused for the program
//!   when some copy the type checker knows reads the value at a type handing
//!   its reference on ([`refusals_at_copies`]), and accepted otherwise.
//!
//! The copies the type checker knows are the classes inheriting a default
//! and the instantiations of a generic class the program builds, a generic
//! body's own included at each type the program reaches it at
//! ([`super::reached_instantiations`]). A site some path reaches a use from
//! still never moves at a copy the type checker does not know: such a copy
//! keeps a funded copy rather than reading a value it moved.

use std::collections::{HashMap, HashSet};

use super::context::{resolve_method_source, MethodSource, TypeDefinition};
use super::generics::generic_parameter_name;
use super::utils::{hands_on_a_reference, runs_drop_hook, Instantiations, ResourceTypes};
use crate::ast::types::{Type, TypeKind, SELF_TYPE_NAME};
use crate::error::type_error::{TypeError, TypeErrorKind};

/// A body compiled once per copy, whose values of an open type the
/// consumption check decides per copy.
#[derive(Debug, Clone)]
pub enum OpenBody {
    /// The default `method` of the trait `trait_name`, whose `self` is
    /// `Self`.
    Default { trait_name: String, method: String },
    /// A method of the generic class `class`, whose own type parameters are
    /// `parameters`, in declaration order.
    GenericClassMethod {
        class: String,
        parameters: Vec<String>,
    },
}

impl OpenBody {
    /// The open parameter a value of `kind` is in this body: `Self` in a
    /// default, a type parameter of the class in a generic class's method.
    /// `None` for any other type, and for a parameter every copy reads as a
    /// resource, a bound running a hook, which is consumed as any resource.
    pub(crate) fn parameter_of(
        &self,
        kind: &TypeKind,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Option<String> {
        let name = generic_parameter_name(kind)?;
        let is_open = match self {
            OpenBody::Default { .. } => name == SELF_TYPE_NAME,
            OpenBody::GenericClassMethod { parameters, .. } => {
                parameters.iter().any(|parameter| parameter == name)
            }
        };
        (is_open && !runs_drop_hook(kind, type_definitions)).then(|| name.to_string())
    }
}

/// A use, after a site consuming a value of an open type, that is refused at
/// every copy reading the value at a type handing its reference on.
#[derive(Debug)]
pub struct OpenRefusal {
    pub error: TypeError,
    pub body: OpenBody,
    /// The open parameter the consumed value's type is.
    pub parameter: String,
}

/// Whether consuming a value a copy reads at `kind` hands its reference on
/// there ([`hands_on_a_reference`]): the one predicate MIR lowering moves a
/// consumed read by and [`refusals_at_copies`] refuses a later use by. A
/// value read at a trait, the type a default's shared body reads `Self` at,
/// is never consumed so: that body runs for classes some of which may not
/// be resources.
pub fn hands_on_at_copy(
    kind: &TypeKind,
    type_definitions: &HashMap<String, TypeDefinition>,
    resources: &ResourceTypes,
) -> bool {
    let is_trait = matches!(kind, TypeKind::Custom(name, _)
        if matches!(type_definitions.get(name), Some(TypeDefinition::Trait(_))));
    !is_trait && hands_on_a_reference(kind, type_definitions, resources)
}

/// What deciding the refusals at the copies reads.
pub(crate) struct Copies<'a> {
    pub type_definitions: &'a HashMap<String, TypeDefinition>,
    pub resources: &'a ResourceTypes,
    pub instantiations: &'a Instantiations,
}

/// Each refusal of `refusals` some known copy reads the consumed value at a
/// type handing its reference on, once per place, naming that copy.
pub(crate) fn refusals_at_copies(
    refusals: Vec<OpenRefusal>,
    copies: &Copies<'_>,
) -> Vec<TypeError> {
    let mut reported = HashSet::new();
    refusals
        .into_iter()
        .filter_map(|refusal| {
            let copy = (copy_types(&refusal, copies).into_iter())
                .find(|ty| hands_on_at_copy(&ty.kind, copies.type_definitions, copies.resources))?;
            reported
                .insert(refusal.error.span)
                .then(|| naming_the_copy(refusal.error, &copy))
        })
        .collect()
}

/// The types the copies the type checker knows read the open parameter of
/// `refusal` at: each class inheriting the default, or the argument each
/// instantiation of the generic class binds the parameter to.
fn copy_types(refusal: &OpenRefusal, copies: &Copies<'_>) -> Vec<Type> {
    let definitions = copies.type_definitions;
    match &refusal.body {
        OpenBody::Default { trait_name, method } => (definitions.iter())
            .filter(|(_, definition)| matches!(definition, TypeDefinition::Class(_)))
            .filter(|(class, _)| {
                matches!(resolve_method_source(definitions, class, method),
                    Some(MethodSource::Default(default)) if default.trait_name == trait_name)
            })
            .map(|(class, _)| crate::ast::factory::make_type(TypeKind::Custom(class.clone(), None)))
            .collect(),
        OpenBody::GenericClassMethod { class, parameters } => {
            let Some(index) = parameters.iter().position(|p| *p == refusal.parameter) else {
                return Vec::new();
            };
            (copies.instantiations.get(class).into_iter().flatten())
                .filter_map(|arguments| arguments.get(index).cloned())
                .collect()
        }
    }
}

/// `error` with the copy it is refused at named after its message.
fn naming_the_copy(mut error: TypeError, copy: &Type) -> TypeError {
    if let TypeErrorKind::Coded { message, .. } = &mut error.kind {
        message.push_str(&format!(
            "\n  note: the body is compiled at `{copy}`, where the value is a resource"
        ));
    }
    error
}
