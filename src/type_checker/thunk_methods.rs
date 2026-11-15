// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The method bodies a container's runtime thunk calls: the one answer to
//! "is this method called by a thunk".
//!
//! A container copies, orders, matches and hashes class elements through
//! their own `clone`, `compare`, `equals` and `hash`, reached through a
//! thunk the runtime calls with the elements it holds. The runtime only
//! lends those elements, so such a body can own nothing it is handed
//! ([`super::kept_parameters`]).
//!
//! This module is the one answer, read by both sides: the code that emits
//! each thunk asks [`is_asked_of`] whether an element class gets one and
//! [`thunk_body`] which body it calls (`mir::type_facts::element_identity`,
//! `mir::dispatch::element_equality`, the pipeline's demand for element
//! bodies, codegen's clone thunk), and the ownership verdict refuses a
//! keeping body among [`ThunkBodies`], the set of every body [`thunk_body`]
//! names for some class. A container asks `clone` of a concrete class
//! implementing `Cloneable`, `compare` of one implementing `Comparable`,
//! `equals` of any class whose `equals` matches an element against another
//! ([`is_element_equality`]), and `hash` of one declaring `equals`. The thunk
//! calls the body the element's own class resolves, by name, never through a
//! vtable. A method of one of those names that no class reaches so is an
//! ordinary method.

use std::collections::{HashMap, HashSet};

use crate::ast::types::{
    is_trait_self, TypeKind, CLONEABLE_TRAIT_NAME, CLONE_METHOD_NAME, EQUALS_METHOD_NAME,
    HASH_METHOD_NAME, ORDERING_METHOD_NAME, ORDERING_TRAIT_NAME,
};

use super::context::{
    class_implements_trait, class_method_declaration, resolve_method_source, MethodInfo,
    MethodSource, TypeDefinition,
};

/// A question a container's runtime thunk asks of a class element, answered
/// by a method of the element's class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThunkQuestion {
    /// A copy of the element: its `clone`.
    Clone,
    /// Which of two elements sorts first: its `compare`.
    Compare,
    /// Whether two elements are the same one: its `equals`.
    Equals,
    /// Where the element is placed in a set or map: its `hash`.
    Hash,
}

impl ThunkQuestion {
    /// Every question a thunk asks.
    pub const ALL: [ThunkQuestion; 4] = [
        ThunkQuestion::Clone,
        ThunkQuestion::Compare,
        ThunkQuestion::Equals,
        ThunkQuestion::Hash,
    ];

    /// The method answering this question.
    pub fn method_name(self) -> &'static str {
        match self {
            ThunkQuestion::Clone => CLONE_METHOD_NAME,
            ThunkQuestion::Compare => ORDERING_METHOD_NAME,
            ThunkQuestion::Equals => EQUALS_METHOD_NAME,
            ThunkQuestion::Hash => HASH_METHOD_NAME,
        }
    }

    /// The question the method `name` answers, when it answers one.
    pub fn answered_by(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|question| question.method_name() == name)
    }
}

/// The bodies a runtime thunk calls: by the class or trait declaring each,
/// which is where a keeping one is refused, and by the class each is
/// compiled under, which is the body the pipeline has to emit for the thunk
/// to call. A class's own or inherited body is compiled under the class
/// declaring it; a trait default under each class it is supplied to.
#[derive(Debug, Default)]
pub struct ThunkBodies {
    declared: HashMap<String, HashSet<&'static str>>,
    compiled: HashMap<String, HashSet<&'static str>>,
}

impl ThunkBodies {
    /// Every body a thunk calls for some class among `type_definitions`
    /// ([`thunk_body`]).
    pub fn of(type_definitions: &HashMap<String, TypeDefinition>) -> Self {
        let classes = (type_definitions.iter())
            .filter(|(_, definition)| matches!(definition, TypeDefinition::Class(_)))
            .map(|(name, _)| name.as_str());
        let mut bodies = ThunkBodies::default();
        for class in classes {
            for question in ThunkQuestion::ALL {
                let Some((declaring, _)) = thunk_body(class, question, type_definitions) else {
                    continue;
                };
                let is_class_body = matches!(
                    type_definitions.get(declaring),
                    Some(TypeDefinition::Class(_))
                );
                let compiled_under = if is_class_body { declaring } else { class };
                let method = question.method_name();
                insert_body(&mut bodies.declared, declaring, method);
                insert_body(&mut bodies.compiled, compiled_under, method);
            }
        }
        bodies
    }

    /// Whether a thunk calls the method `method` the class or trait `owner`
    /// declares.
    pub fn contains(&self, owner: &str, method: &str) -> bool {
        (self.declared.get(owner)).is_some_and(|methods| methods.contains(method))
    }

    /// Whether a thunk calls the body of `method` compiled under the class
    /// `class`: one the class declares, or a trait default supplied to it.
    pub fn is_compiled_under(&self, class: &str, method: &str) -> bool {
        (self.compiled.get(class)).is_some_and(|methods| methods.contains(method))
    }
}

fn insert_body(
    bodies: &mut HashMap<String, HashSet<&'static str>>,
    owner: &str,
    method: &'static str,
) {
    bodies.entry(owner.to_string()).or_default().insert(method);
}

/// The body a container's thunk asking `question` of a `class` element
/// calls, with the class or trait declaring it: the body `class` resolves
/// for the method, when the container asks the question of it
/// ([`is_asked_of`]) and that body answers it. `None` when no thunk calls
/// one.
pub fn thunk_body<'d>(
    class: &str,
    question: ThunkQuestion,
    type_definitions: &'d HashMap<String, TypeDefinition>,
) -> Option<(&'d str, &'d MethodInfo)> {
    if !is_asked_of(class, question, type_definitions) {
        return None;
    }
    let method = question.method_name();
    let (declaring, info) = match resolve_method_source(type_definitions, class, method)? {
        MethodSource::Declared { class, info } => (class, info),
        MethodSource::Default(default) => (default.trait_name, default.info),
        MethodSource::AbstractOnly { .. } => return None,
    };
    let is_answered = match question {
        ThunkQuestion::Equals => is_element_equality(declaring, info),
        ThunkQuestion::Clone | ThunkQuestion::Compare | ThunkQuestion::Hash => true,
    };
    is_answered.then_some((declaring, info))
}

/// Whether a container asks a `class` element `question`, so a thunk for it
/// exists: a copy of a concrete class implementing `Cloneable` (an abstract
/// one has no instance of its own to copy), the order of a class
/// implementing `Comparable`, the match of any class, and the hash of a
/// class declaring `equals`. A hash thunk exists for such a class whether or
/// not it declares `hash`; without one it hashes the instance. The thunk is
/// the class's own `hash` symbol, so the runtime calls whichever `hash` the
/// class declares, whatever its shape.
pub fn is_asked_of(
    class: &str,
    question: ThunkQuestion,
    type_definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    let Some(TypeDefinition::Class(definition)) = type_definitions.get(class) else {
        return false;
    };
    match question {
        ThunkQuestion::Clone => {
            !definition.is_abstract
                && class_implements_trait(class, CLONEABLE_TRAIT_NAME, type_definitions)
        }
        ThunkQuestion::Compare => {
            class_implements_trait(class, ORDERING_TRAIT_NAME, type_definitions)
        }
        ThunkQuestion::Equals => true,
        ThunkQuestion::Hash => {
            class_method_declaration(class, EQUALS_METHOD_NAME, type_definitions).is_some()
        }
    }
}

/// Whether `method`, the `equals` `type_name` writes, is the equality a
/// container matches two of its elements by: an instance method taking one
/// value of the element's own type and answering with a boolean. A trait
/// default declares that value as the trait's `Self`, which is the class
/// the default is lowered for.
///
/// The thunk hands the method two element pointers, so a method declared over
/// some other parameter type would read an element as a value it is not.
pub fn is_element_equality(type_name: &str, method: &MethodInfo) -> bool {
    let [(_, param_ty)] = method.params.as_slice() else {
        return false;
    };
    let takes_own_type = matches!(
        &param_ty.kind,
        TypeKind::Custom(name, _) if name == type_name
    ) || is_trait_self(&param_ty.kind);
    !method.is_abstract
        && !method.is_static
        && takes_own_type
        && matches!(method.return_type.kind, TypeKind::Boolean)
}
