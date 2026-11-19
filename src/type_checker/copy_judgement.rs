// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A collection copies an element only through the element's own `clone`.
//!
//! A `runtime` entry point declared `@clones` stores in its collection a copy
//! of an element the caller lends, and one returning a copy of a collection
//! (`@copies`) copies every element of it: the copy the element's `clone`
//! makes where the collection registered one, the element shared otherwise.
//! Sharing is sound for a value that is no resource, which copy-on-write
//! keeps apart, and unsound for a resource, whose hook would run once for two
//! holders. So a copy is refused at an element type that is a resource the
//! collection shares ([`refuses_copy_of`], reading [`element_copy`], the one
//! answer to how a container copies an element): a class that does not
//! implement the copy trait (the trait declared `@copy_trait`), an abstract
//! one, any resource that is neither a class nor a collection (a struct or an
//! enum with a hook), and a list, set or map holding a resource refused so.
//! A key of a map is never cloned, so a resource key is refused too, in a
//! map copied itself or held by another collection.
//!
//! A copy made in a body compiled once per copy, of a value of the body's
//! open type parameter ([`OpenCopy`]), is judged at every copy of the body
//! the type checker knows ([`open_consumption::copy_types`]). A refusal is
//! placed where the program reaches the chain of bodies leading to the copy,
//! and its note names the line making it. The same predicate refuses a copy
//! MIR lowering is asked to make at a concrete type, so a copy no judgement
//! saw still never compiles.

use std::collections::{HashMap, HashSet};

use crate::ast::type_arguments::{argument_type_marked, TypeArgument};
use crate::ast::types::{BuiltinCollectionKind, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::RelatedNote;
use crate::error::syntax::{find_line_info, Span};
use crate::error::type_error::{TypeError, TypeErrorKind};
use crate::runtime_effects::ElementRole;

use super::context::{self, TypeDefinition};
use super::open_consumption::{copy_types, Copies, OpenBody};
use super::thunk_methods::{thunk_body, ThunkQuestion};
use super::utils::ResourceTypes;

/// A copy a body compiled once per copy makes of a value of its open type
/// parameter, judged at each copy of the body the type checker knows.
#[derive(Debug, Clone)]
pub struct OpenCopy {
    /// The body making the copy.
    pub body: OpenBody,
    /// The open parameter the copied value's type is.
    pub parameter: String,
    /// What the copied value is to the collection.
    pub role: ElementRole,
    /// Where the body calls the entry point making the copy.
    pub span: Span,
    /// The imported file `span` is in; `None` for the program's own.
    pub file: Option<String>,
}

/// How a collection copies one of its elements: the one answer read by the
/// copy judgement here and by the code registering each collection's clone
/// callback (`TypeFacts::element_copy`), so a copy is refused exactly where
/// no callback copies the element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementCopy<'k> {
    /// The copy shares the element: sound for a value that is no resource,
    /// which copy-on-write keeps apart.
    Shared,
    /// Through the clone thunk of this class, which calls its `clone`
    /// ([`thunk_body`]).
    Class(&'k str),
    /// Through the runtime's copy of this built-in collection, which copies
    /// each element it holds through the callback the collection was built
    /// with, and shares each key.
    Collection(CopiedCollection),
}

/// A built-in collection the runtime copies when it is held as an element.
/// An array is not one: an inner array is shared ([`element_copy`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopiedCollection {
    List,
    Map,
    Set,
}

/// How a collection copies an element of `kind`: a class through its clone
/// thunk, a list, set or map one of whose elements or values is copied so
/// through the runtime's copy of it, anything else shared.
///
/// TODO: an inner array is shared even where its elements are copied
/// through their `clone`, and an array holding a resource is not judged a
/// resource ([`ResourceTypes::holds`]), so a copy of a list of arrays of a
/// resource shares each resource unrefused. Missing: an array of resources
/// judged a resource, and the runtime's array copy registered as an
/// element's callback.
pub fn element_copy<'k>(
    kind: &'k TypeKind,
    definitions: &HashMap<String, TypeDefinition>,
) -> ElementCopy<'k> {
    let Some(collection) = kind.as_builtin_collection() else {
        if let TypeKind::Custom(name, _) = kind {
            if thunk_body(name, ThunkQuestion::Clone, definitions).is_some() {
                return ElementCopy::Class(name);
            }
        }
        return ElementCopy::Shared;
    };
    let copies_an_element = (copied_element_kinds(kind, definitions).iter()).any(|(kind, role)| {
        *role != ElementRole::Key && element_copy(kind, definitions) != ElementCopy::Shared
    });
    let copied = match collection {
        BuiltinCollectionKind::List => CopiedCollection::List,
        BuiltinCollectionKind::Map => CopiedCollection::Map,
        BuiltinCollectionKind::Set => CopiedCollection::Set,
        BuiltinCollectionKind::Array => return ElementCopy::Shared,
    };
    if copies_an_element {
        ElementCopy::Collection(copied)
    } else {
        ElementCopy::Shared
    }
}

/// Whether a collection refuses to copy an element of `kind` stored as
/// `role`: a resource it shares rather than copies ([`element_copy`]), or
/// one of a key, which is never copied. An inner collection is copied by the
/// runtime with the elements it holds, so it is refused where one of those
/// is.
pub fn refuses_copy_of(
    kind: &TypeKind,
    role: ElementRole,
    definitions: &HashMap<String, TypeDefinition>,
    resources: &ResourceTypes,
) -> bool {
    if !resources.holds(kind) {
        return false;
    }
    match role {
        ElementRole::Key => true,
        ElementRole::Element | ElementRole::Value | ElementRole::Parameter => {
            match element_copy(kind, definitions) {
                ElementCopy::Shared => true,
                ElementCopy::Class(_) => false,
                ElementCopy::Collection(_) => (copied_element_kinds(kind, definitions).iter())
                    .any(|(element, role)| refuses_copy_of(element, *role, definitions, resources)),
            }
        }
    }
}

/// The values a copy of a collection of `kind` copies, each with what it is
/// to the collection: each type it is an instance at, or each of its type's
/// parameters where it names none. A map's first is its key.
pub fn copied_element_kinds(
    kind: &TypeKind,
    definitions: &HashMap<String, TypeDefinition>,
) -> Vec<(TypeKind, ElementRole)> {
    let kinds: Vec<TypeKind> = if let Some(arguments) = written_arguments(kind) {
        (arguments.into_iter())
            .filter_map(argument_type_marked)
            .map(|ty| ty.kind)
            .collect()
    } else if let TypeKind::Custom(name, None) = kind {
        (definitions.get(name))
            .and_then(TypeDefinition::generics)
            .unwrap_or_default()
            .iter()
            .map(|generic| TypeKind::Custom(generic.name.clone(), None))
            .collect()
    } else {
        Vec::new()
    };
    let collection = kind.as_builtin_collection();
    (kinds.into_iter().enumerate())
        .map(|(position, kind)| (kind, argument_role(collection, position)))
        .collect()
}

/// The type arguments `kind` is written with, in either spelling of a
/// built-in collection; `None` where it names none.
fn written_arguments(kind: &TypeKind) -> Option<Vec<&TypeArgument>> {
    if let TypeKind::Custom(_, arguments) = kind {
        arguments
            .as_ref()
            .map(|arguments| arguments.iter().collect())
    } else if let TypeKind::List(element) | TypeKind::Set(element) = kind {
        Some(vec![&**element])
    } else if let TypeKind::Map(first, second) | TypeKind::Array(first, second) = kind {
        Some(vec![&**first, &**second])
    } else {
        None
    }
}

/// What the type argument at `position` of `collection` is to it: a map's
/// key, then its value; any other collection's element.
fn argument_role(collection: Option<BuiltinCollectionKind>, position: usize) -> ElementRole {
    match collection {
        Some(BuiltinCollectionKind::Map) if position == 0 => ElementRole::Key,
        Some(BuiltinCollectionKind::Map) => ElementRole::Value,
        Some(
            BuiltinCollectionKind::Array | BuiltinCollectionKind::List | BuiltinCollectionKind::Set,
        )
        | None => ElementRole::Element,
    }
}

/// A copy refused at an element type, and the copy trait its repair names.
#[derive(Debug, Clone, Copy)]
pub struct RefusedCopy<'a> {
    /// The element type: a resource with no `clone`.
    pub element: &'a TypeKind,
    /// The copy trait, where a module declares one.
    pub copy_trait: Option<&'a str>,
}

impl<'a> RefusedCopy<'a> {
    /// The refusal of a copy of `element`. Its repair names the copy trait
    /// only where the element's type can declare it, which a class can and
    /// an enum, a struct or an optional cannot; for those, handing each
    /// value over is the one repair that compiles.
    pub fn of(element: &'a TypeKind, definitions: &'a HashMap<String, TypeDefinition>) -> Self {
        let can_declare = matches!(element, TypeKind::Custom(name, _)
            if matches!(definitions.get(name), Some(TypeDefinition::Class(_))));
        let copy_trait = can_declare
            .then(|| context::copy_trait(definitions))
            .flatten();
        RefusedCopy {
            element,
            copy_trait,
        }
    }
}

/// The refusal of `refused`, placed at `span`. `copier` names the method the
/// program calls when the copy is made inside it; `related` says where.
pub(crate) fn copy_refusal(
    refused: RefusedCopy<'_>,
    copier: Option<String>,
    span: Span,
    related: Vec<RelatedNote>,
) -> TypeError {
    let RefusedCopy {
        element,
        copy_trait,
    } = refused;
    let message = match copier {
        Some(copier) => format!(
            "`{copier}` copies elements of `{element}`, a resource with no `clone` to copy it with"
        ),
        None => format!("this call copies `{element}`, a resource with no `clone` to copy it with"),
    };
    TypeError {
        kind: TypeErrorKind::Coded {
            code: DiagnosticCode::OwnResourceElementCopied,
            message,
            help: Some(copy_repair(element, copy_trait)),
            expected: None,
            actual: None,
        },
        span,
        source_override: None,
        repair: None,
        related,
    }
}

/// The repair of a copy of `element`: implement the copy trait
/// `copy_trait` where one is declared, or hand each value over.
fn copy_repair(element: &TypeKind, copy_trait: Option<&str>) -> String {
    match copy_trait {
        Some(copy_trait) => format!(
            "implement `{copy_trait}` on `{element}` so each copy is its own value, or hand \
             each value over instead of copying it"
        ),
        None => "hand each value over instead of copying it".to_string(),
    }
}

/// The refusal of each copy among `copies` some known copy of its body makes
/// at a refused element type, once per place and element type. `source_of`
/// gives the text of an imported file by its path.
pub(crate) fn refusals_at_known_copies<'s>(
    copies: &[OpenCopy],
    known: &Copies<'_>,
    source_of: impl Fn(&str) -> Option<&'s str>,
) -> Vec<TypeError> {
    let mut reported = HashSet::new();
    let mut refusals = Vec::new();
    for copy in copies {
        for at in copy_types(&copy.body, &copy.parameter, known) {
            let element = &at.ty.kind;
            if !refuses_copy_of(element, copy.role, known.type_definitions, known.resources) {
                continue;
            }
            let site = at
                .reached
                .map_or(copy.span, |reached| reached.program_site());
            if reported.insert((site, element.clone())) {
                let refused = RefusedCopy::of(element, known.type_definitions);
                refusals.push(refusal_at(copy, refused, at.reached, &source_of));
            }
        }
    }
    refusals
}

/// The refusal of `copy`, refused as `refused`.
///
/// When the program reaches the copy, the refusal sits at the program's site
/// of `reached`. It names the method the program calls and the line that
/// makes the copy. When nothing in the program reaches it, the refusal sits
/// at the copy itself, in its own file. A default method a class inherits is
/// such a copy.
fn refusal_at<'s>(
    copy: &OpenCopy,
    refused: RefusedCopy<'_>,
    reached: Option<&super::reached_instantiations::ReachedCopy>,
    source_of: &impl Fn(&str) -> Option<&'s str>,
) -> TypeError {
    let text = copy.file.as_deref().and_then(source_of);
    let Some(reached) = reached else {
        let mut refusal = copy_refusal(refused, None, copy.span, Vec::new());
        refusal.source_override = (copy.file.clone()).zip(text.map(str::to_string));
        return refusal;
    };
    let (declared_in, method) = reached.body();
    let (owner, name) =
        (reached.reached_through().last()).unwrap_or((declared_in.as_str(), method.as_str()));
    let related = (copy.file.as_deref().zip(text))
        .map(|(path, text)| {
            let (line, _, _) = find_line_info(text, copy.span.start);
            RelatedNote {
                message: format!("the copy is made at {path}:{line}, in `{declared_in}.{method}`"),
                span: None,
            }
        })
        .into_iter()
        .collect();
    copy_refusal(
        refused,
        Some(format!("{owner}.{name}")),
        reached.program_site(),
        related,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_copy_repair_without_a_copy_trait_hands_each_value_over() {
        let definitions = HashMap::new();
        let element = TypeKind::Custom("Res".to_string(), None);
        let repair = copy_repair(&element, context::copy_trait(&definitions));
        assert_eq!(repair, "hand each value over instead of copying it");
    }
}
