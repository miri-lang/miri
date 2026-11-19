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
//! holders. So a copy is refused at an element type that is a resource with
//! no `clone` thunk ([`refuses_copy_of`], reading
//! [`thunk_methods::is_asked_of`], the one answer to whether a container
//! clones a class's elements): a class that does not implement the copy
//! trait (the trait declared `@copy_trait`),
//! an abstract one, and any resource that is not a class (a struct or an
//! enum with a hook, a collection of resources). A key of a map is never
//! cloned, so a resource key is refused too.
//!
//! A copy made in a body compiled once per copy, of a value of the body's
//! open type parameter ([`OpenCopy`]), is judged at every copy of the body
//! the type checker knows ([`open_consumption::copy_types`]). A refusal is
//! placed where the program reaches the chain of bodies leading to the copy,
//! and its note names the line making it. The same predicate refuses a copy
//! MIR lowering is asked to make at a concrete type, so a copy no judgement
//! saw still never compiles.

use std::collections::{HashMap, HashSet};

use crate::ast::type_arguments::argument_type_marked;
use crate::ast::types::TypeKind;
use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::RelatedNote;
use crate::error::syntax::{find_line_info, Span};
use crate::error::type_error::{TypeError, TypeErrorKind};
use crate::runtime_effects::ElementRole;

use super::context::{self, TypeDefinition};
use super::open_consumption::{copy_types, Copies, OpenBody};
use super::thunk_methods::{is_asked_of, ThunkQuestion};
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

/// Whether a collection refuses to copy an element of `kind` stored as
/// `role`: a resource it has no `clone` to copy with. An inner collection is
/// copied by the runtime with the elements it holds, so it is refused where
/// one of those is.
pub fn refuses_copy_of(
    kind: &TypeKind,
    role: ElementRole,
    definitions: &HashMap<String, TypeDefinition>,
    resources: &ResourceTypes,
) -> bool {
    if !resources.holds(kind) {
        return false;
    }
    if kind.as_builtin_collection().is_some() {
        let elements = copied_element_kinds(kind, definitions);
        return (elements.iter())
            .any(|element| refuses_copy_of(element, role, definitions, resources));
    }
    let is_cloned_class = matches!(kind, TypeKind::Custom(class, _)
        if is_asked_of(class, ThunkQuestion::Clone, definitions));
    match role {
        ElementRole::Element | ElementRole::Value | ElementRole::Parameter => !is_cloned_class,
        ElementRole::Key => true,
    }
}

/// The element types a copy of a collection of `kind` copies a value of:
/// each type it is an instance at, or each of its type's parameters where it
/// names none.
pub fn copied_element_kinds(
    kind: &TypeKind,
    definitions: &HashMap<String, TypeDefinition>,
) -> Vec<TypeKind> {
    let TypeKind::Custom(name, arguments) = kind else {
        return Vec::new();
    };
    match arguments {
        Some(arguments) => (arguments.iter())
            .filter_map(argument_type_marked)
            .map(|ty| ty.kind)
            .collect(),
        None => (definitions.get(name))
            .and_then(TypeDefinition::generics)
            .unwrap_or_default()
            .iter()
            .map(|generic| TypeKind::Custom(generic.name.clone(), None))
            .collect(),
    }
}

/// A copy refused at an element type, and the copy trait its repair names.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RefusedCopy<'a> {
    /// The element type: a resource with no `clone`.
    pub element: &'a TypeKind,
    /// The copy trait, where a module declares one.
    pub copy_trait: Option<&'a str>,
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
    let copy_trait = context::copy_trait(known.type_definitions);
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
                let refused = RefusedCopy {
                    element,
                    copy_trait,
                };
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
