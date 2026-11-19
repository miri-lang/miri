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
//! The copies the type checker knows are the classes inheriting a default,
//! each instantiation of a generic class's method the program reaches, and
//! each set of types the program reaches a function or a method declaring
//! type parameters of its own at
//! ([`super::reached_instantiations::ReachedCopy`]): a library collection's
//! method is judged only where the program calls it. A site some path
//! reaches a use from still never moves at a copy the type checker does not
//! know: such a copy keeps a funded copy rather than reading a value it
//! moved.

use std::collections::{HashMap, HashSet};

use super::context::{resolve_method_source, MethodSource, TypeDefinition};
use super::escape_analysis::FunctionId;
use super::generics::generic_parameter_name;
use super::instantiation_requirements::{free_function_owner, GenericBodyId};
use super::reached_instantiations::{declared_parameter_name, ReachedCopy};
use super::utils::{hands_on_a_reference, runs_drop_hook, ResourceTypes};
use crate::ast::statement::FunctionDeclarationData;
use crate::ast::types::{Type, TypeKind, SELF_TYPE_NAME};
use crate::error::type_error::{TypeError, TypeErrorKind};

/// A body compiled once per copy, whose values of an open type the
/// consumption check decides per copy.
#[derive(Debug, Clone)]
pub enum OpenBody {
    /// The default `method` of the trait `trait_name`, whose `self` is
    /// `Self`.
    Default { trait_name: String, method: String },
    /// The method `method` of the generic class `class`, whose own type
    /// parameters are `parameters`, in declaration order.
    GenericClassMethod {
        class: String,
        method: String,
        parameters: Vec<String>,
    },
    /// A function or a method declaring type parameters of its own,
    /// `parameters` in declaration order, compiled once per set of types
    /// the program reaches `body` at. A method's `owner` is the body its
    /// type's parameters, or a default's `Self`, are open in.
    OwnParameters {
        body: GenericBodyId,
        parameters: Vec<String>,
        owner: Option<Box<OpenBody>>,
    },
}

impl OpenBody {
    /// The body the method `method` of the type `owner` is, when the type
    /// declares type parameters: compiled once per instantiation.
    pub(crate) fn of_generic_owner(
        (owner, method): (&str, &str),
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Option<Self> {
        let parameters: Vec<String> = (type_definitions.get(owner)?.generics()?.iter())
            .map(|generic| generic.name.clone())
            .collect();
        (!parameters.is_empty()).then(|| OpenBody::GenericClassMethod {
            class: owner.to_string(),
            method: method.to_string(),
            parameters,
        })
    }

    /// The body `decl`, declared as `function`, is: one declaring type
    /// parameters of its own is compiled once per set of types the program
    /// reaches it at, its owner's parameters open as `owner` says; any other
    /// is `owner`.
    pub(crate) fn of_declaration(
        function: &FunctionId,
        decl: &FunctionDeclarationData,
        owner: Option<Self>,
    ) -> Option<Self> {
        let parameters: Vec<String> = (decl.generics.iter().flatten())
            .filter_map(declared_parameter_name)
            .map(str::to_string)
            .collect();
        if parameters.is_empty() {
            return owner;
        }
        let body = match function {
            FunctionId::Declared(declared) => {
                (free_function_owner(&declared.module), declared.name.clone())
            }
            FunctionId::Method { owner, method } => (owner.clone(), method.clone()),
            FunctionId::Free(_) => return owner,
        };
        Some(OpenBody::OwnParameters {
            body,
            parameters,
            owner: owner.map(Box::new),
        })
    }

    /// The open parameter a value of `kind` is in this body: `Self` in a
    /// default, a type parameter of the class in a generic class's method,
    /// one of the body's own. `None` for any other type, and for a
    /// parameter every copy reads as a resource, a bound running a hook,
    /// which is consumed as any resource.
    pub(crate) fn parameter_of(
        &self,
        kind: &TypeKind,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Option<String> {
        let name = generic_parameter_name(kind)?;
        (self.opens(name) && !runs_drop_hook(kind, type_definitions)).then(|| name.to_string())
    }

    /// Whether the type named `name` is open in this body.
    fn opens(&self, name: &str) -> bool {
        match self {
            OpenBody::Default { .. } => name == SELF_TYPE_NAME,
            OpenBody::GenericClassMethod { parameters, .. } => {
                parameters.iter().any(|parameter| parameter == name)
            }
            OpenBody::OwnParameters {
                parameters, owner, ..
            } => {
                parameters.iter().any(|parameter| parameter == name)
                    || owner.as_deref().is_some_and(|owner| owner.opens(name))
            }
        }
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
    /// Every body the program reaches, at each set of types it is reached
    /// at.
    pub reached: &'a [ReachedCopy],
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
            let copy = (copy_types(&refusal.body, &refusal.parameter, copies).into_iter())
                .map(|known| known.ty)
                .find(|ty| hands_on_at_copy(&ty.kind, copies.type_definitions, copies.resources))?;
            reported
                .insert(refusal.error.span)
                .then(|| naming_the_copy(refusal.error, &copy))
        })
        .collect()
}

/// One copy of a body the type checker knows, and the type it reads an open
/// parameter at.
pub(crate) struct KnownCopy<'a> {
    pub ty: Type,
    /// The copy the program reaches, for a body compiled once per set of
    /// types it is reached at; `None` for a class inheriting a default.
    pub reached: Option<&'a ReachedCopy>,
}

/// The copies of `body` the type checker knows, each with the type it reads
/// the open parameter `parameter` at: each class inheriting the default, or
/// each copy of a generic class's method, or of a body declaring the
/// parameter, the program reaches, at the type it binds the parameter to.
pub(crate) fn copy_types<'a>(
    body: &OpenBody,
    parameter: &str,
    copies: &Copies<'a>,
) -> Vec<KnownCopy<'a>> {
    let definitions = copies.type_definitions;
    let reached_at = |is_copy: &dyn Fn(&ReachedCopy) -> bool| {
        (copies.reached.iter())
            .filter(|copy| is_copy(copy))
            .filter_map(|copy| {
                let ty = copy.bound(parameter)?.clone();
                Some(KnownCopy {
                    ty,
                    reached: Some(copy),
                })
            })
            .collect()
    };
    match body {
        OpenBody::Default { trait_name, method } => (definitions.iter())
            .filter(|(_, definition)| matches!(definition, TypeDefinition::Class(_)))
            .filter(|(class, _)| {
                matches!(resolve_method_source(definitions, class, method),
                    Some(MethodSource::Default(default)) if default.trait_name == trait_name)
            })
            .map(|(class, _)| KnownCopy {
                ty: crate::ast::factory::make_type(TypeKind::Custom(class.clone(), None)),
                reached: None,
            })
            .collect(),
        OpenBody::GenericClassMethod { class, method, .. } => {
            reached_at(&|copy| copy.body().0 == *class && copy.body().1 == *method)
        }
        OpenBody::OwnParameters {
            body,
            parameters,
            owner,
        } => match owner {
            Some(owner) if !parameters.iter().any(|p| p == parameter) => {
                copy_types(owner, parameter, copies)
            }
            Some(_) | None => reached_at(&|copy| copy.body() == body),
        },
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
