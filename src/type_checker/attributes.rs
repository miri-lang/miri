// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The closed registry of compiler-known attributes, and validation against it.
//!
//! Attributes are markers the compiler understands, not macros: there are no
//! user-defined attributes and no expansion. An unrecognized attribute is a
//! hard error rather than a silently ignored annotation.

use crate::ast::{
    Attribute, AttributeTarget, DEPRECATED_ATTRIBUTE, MUST_USE_ATTRIBUTE, NON_EXHAUSTIVE_ATTRIBUTE,
};
use crate::error::type_error::{TypeError, TypeErrorKind};

/// Registry entry describing where one attribute may appear and what it carries.
pub(crate) struct AttributeSpec {
    pub name: &'static str,
    pub valid_on: &'static [AttributeTarget],
    pub takes_argument: bool,
}

/// Every attribute the compiler knows. Adding one is a single entry here plus
/// the semantic hook that consumes it.
const ATTRIBUTE_REGISTRY: &[AttributeSpec] = &[
    AttributeSpec {
        name: NON_EXHAUSTIVE_ATTRIBUTE,
        valid_on: &[AttributeTarget::Enum],
        takes_argument: false,
    },
    AttributeSpec {
        name: MUST_USE_ATTRIBUTE,
        valid_on: &[AttributeTarget::Enum],
        takes_argument: false,
    },
    AttributeSpec {
        name: DEPRECATED_ATTRIBUTE,
        valid_on: &[AttributeTarget::Function],
        takes_argument: true,
    }, // Note: class/enum deprecation would require a call-site hook at
       // instantiation, which is not yet implemented. This is fail-closed for v1.
];

/// Returns every attribute a declaration of this kind accepts, for diagnostics.
fn attributes_valid_on(target: AttributeTarget) -> Vec<String> {
    ATTRIBUTE_REGISTRY
        .iter()
        .filter(|spec| spec.valid_on.contains(&target))
        .map(|spec| format!("@{}", spec.name))
        .collect()
}

fn known_attribute_names() -> Vec<String> {
    ATTRIBUTE_REGISTRY
        .iter()
        .map(|spec| format!("@{}", spec.name))
        .collect()
}

/// Validates every attribute on one declaration, returning all violations so a
/// declaration carrying several bad attributes reports them in one pass.
pub(crate) fn validate_attributes(
    attributes: &[Attribute],
    target: AttributeTarget,
) -> Vec<TypeError> {
    attributes
        .iter()
        .filter_map(|attribute| validate_attribute(attribute, target))
        .collect()
}

fn validate_attribute(attribute: &Attribute, target: AttributeTarget) -> Option<TypeError> {
    let Some(spec) = ATTRIBUTE_REGISTRY
        .iter()
        .find(|spec| spec.name == attribute.name)
    else {
        return Some(TypeError::new(
            TypeErrorKind::UnknownAttribute {
                name: attribute.name.clone(),
                known: known_attribute_names(),
            },
            attribute.span,
        ));
    };

    if !spec.valid_on.contains(&target) {
        return Some(TypeError::new(
            TypeErrorKind::AttributeNotValid {
                name: attribute.name.clone(),
                target: target.to_string(),
                accepted: attributes_valid_on(target),
            },
            attribute.span,
        ));
    }

    match (spec.takes_argument, attribute.argument.is_some()) {
        (true, false) => Some(TypeError::new(
            TypeErrorKind::AttributeArgumentMissing {
                name: attribute.name.clone(),
            },
            attribute.span,
        )),
        (false, true) => Some(TypeError::new(
            TypeErrorKind::AttributeArgumentExtra {
                name: attribute.name.clone(),
            },
            attribute.span,
        )),
        (true, true) | (false, false) => None,
    }
}

/// True if the declaration carries the named attribute.
pub(crate) fn has_attribute(attributes: &[Attribute], name: &str) -> bool {
    attributes.iter().any(|attribute| attribute.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::AttributeSpelling;
    use crate::error::syntax::Span;

    fn attribute(name: &str, argument: Option<&str>) -> Attribute {
        Attribute::new(
            name.to_string(),
            argument.map(str::to_string),
            Span::default(),
            AttributeSpelling::Attribute,
        )
    }

    #[test]
    fn accepts_a_registered_attribute_on_its_target() {
        let attributes = [attribute(MUST_USE_ATTRIBUTE, None)];
        assert!(validate_attributes(&attributes, AttributeTarget::Enum).is_empty());
    }

    #[test]
    fn rejects_an_unregistered_attribute() {
        let attributes = [attribute("nonexistent", None)];
        let errors = validate_attributes(&attributes, AttributeTarget::Enum);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind.properties().code, "E0112");
    }

    #[test]
    fn rejects_an_attribute_on_the_wrong_declaration_kind() {
        let attributes = [attribute(NON_EXHAUSTIVE_ATTRIBUTE, None)];
        let errors = validate_attributes(&attributes, AttributeTarget::Function);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind.properties().code, "E0113");
    }

    #[test]
    fn rejects_an_argument_the_attribute_does_not_take() {
        let attributes = [attribute(MUST_USE_ATTRIBUTE, Some("why"))];
        let errors = validate_attributes(&attributes, AttributeTarget::Enum);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind.properties().code, "E0114");
    }

    #[test]
    fn reports_every_violation_on_one_declaration() {
        let attributes = [
            attribute("nonexistent", None),
            attribute(MUST_USE_ATTRIBUTE, Some("why")),
        ];
        let errors = validate_attributes(&attributes, AttributeTarget::Enum);
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn rejects_deprecated_without_argument() {
        let attributes = [attribute(DEPRECATED_ATTRIBUTE, None)];
        let errors = validate_attributes(&attributes, AttributeTarget::Function);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind.properties().code, "E0114");
    }

    #[test]
    fn accepts_deprecated_with_argument_on_function() {
        let attributes = [attribute(DEPRECATED_ATTRIBUTE, Some("use X instead"))];
        assert!(validate_attributes(&attributes, AttributeTarget::Function).is_empty());
    }

    #[test]
    fn rejects_deprecated_on_class_and_enum() {
        let attributes = [attribute(DEPRECATED_ATTRIBUTE, Some("reason"))];
        let class_errors = validate_attributes(&attributes, AttributeTarget::Class);
        assert_eq!(class_errors.len(), 1);
        assert_eq!(class_errors[0].kind.properties().code, "E0113");

        let enum_errors = validate_attributes(&attributes, AttributeTarget::Enum);
        assert_eq!(enum_errors.len(), 1);
        assert_eq!(enum_errors[0].kind.properties().code, "E0113");
    }
}
