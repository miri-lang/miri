// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The closed registry of compiler-known attributes, and validation against it.
//!
//! Attributes are markers the compiler understands, not macros: there are no
//! user-defined attributes and no expansion. An unrecognized attribute is a
//! hard error rather than a silently ignored annotation.

use crate::ast::{
    Attribute, AttributeTarget, DEPRECATED_ATTRIBUTE, IGNORE_ATTRIBUTE, MUST_USE_ATTRIBUTE,
    NON_EXHAUSTIVE_ATTRIBUTE, TEST_ATTRIBUTE, XFAIL_ATTRIBUTE,
};
use crate::error::type_error::{TypeError, TypeErrorKind};

/// What a `@deprecated` declaration is, and the reason its author gave.
///
/// The kind selects the noun the use-site warning uses, so a deprecated class
/// does not report itself as a deprecated function.
#[derive(Debug, Clone)]
pub(crate) struct Deprecation {
    pub kind: DeprecatedKind,
    pub reason: String,
}

/// The kind of declaration a deprecation applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeprecatedKind {
    Function,
    Class,
    Enum,
}

impl DeprecatedKind {
    /// The noun used to describe this kind in a diagnostic.
    pub fn noun(self) -> &'static str {
        match self {
            DeprecatedKind::Function => "function",
            DeprecatedKind::Class => "class",
            DeprecatedKind::Enum => "enum",
        }
    }

    /// Warning title for a use of a declaration of this kind.
    pub fn warning_title(self) -> String {
        match self {
            DeprecatedKind::Function => "Deprecated Function".to_string(),
            DeprecatedKind::Class => "Deprecated Class".to_string(),
            DeprecatedKind::Enum => "Deprecated Enum".to_string(),
        }
    }
}

/// Registry entry describing where one attribute may appear and what it carries.
pub(crate) struct AttributeSpec {
    pub name: &'static str,
    pub valid_on: &'static [AttributeTarget],
    pub takes_argument: bool,
    /// If non-empty, this attribute requires all of these companion attributes
    /// to be present on the same declaration.
    pub requires: &'static [&'static str],
}

/// Every attribute the compiler knows. Adding one is a single entry here plus
/// the semantic hook that consumes it.
const ATTRIBUTE_REGISTRY: &[AttributeSpec] = &[
    AttributeSpec {
        name: NON_EXHAUSTIVE_ATTRIBUTE,
        valid_on: &[AttributeTarget::Enum],
        takes_argument: false,
        requires: &[],
    },
    AttributeSpec {
        name: MUST_USE_ATTRIBUTE,
        valid_on: &[AttributeTarget::Enum],
        takes_argument: false,
        requires: &[],
    },
    AttributeSpec {
        name: DEPRECATED_ATTRIBUTE,
        valid_on: &[
            AttributeTarget::Function,
            AttributeTarget::Class,
            AttributeTarget::Enum,
        ],
        takes_argument: true,
        requires: &[],
    },
    AttributeSpec {
        name: TEST_ATTRIBUTE,
        valid_on: &[AttributeTarget::Function],
        takes_argument: false,
        requires: &[],
    },
    AttributeSpec {
        name: IGNORE_ATTRIBUTE,
        valid_on: &[AttributeTarget::Function],
        takes_argument: true,
        requires: &[TEST_ATTRIBUTE],
    },
    AttributeSpec {
        name: XFAIL_ATTRIBUTE,
        valid_on: &[AttributeTarget::Function],
        takes_argument: true,
        requires: &[TEST_ATTRIBUTE],
    },
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
        .filter_map(|attribute| validate_attribute(attribute, target, attributes))
        .collect()
}

fn validate_attribute(
    attribute: &Attribute,
    target: AttributeTarget,
    all_attributes: &[Attribute],
) -> Option<TypeError> {
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
        (true, false) => {
            return Some(TypeError::new(
                TypeErrorKind::AttributeArgumentMissing {
                    name: attribute.name.clone(),
                },
                attribute.span,
            ))
        }
        (false, true) => {
            return Some(TypeError::new(
                TypeErrorKind::AttributeArgumentExtra {
                    name: attribute.name.clone(),
                },
                attribute.span,
            ))
        }
        (true, true) | (false, false) => {}
    }

    // Check companion requirements
    for required_name in spec.requires {
        if !has_attribute(all_attributes, required_name) {
            return Some(TypeError::new(
                TypeErrorKind::MissingRequiredAttribute {
                    attribute_name: attribute.name.clone(),
                    required_attribute: required_name.to_string(),
                },
                attribute.span,
            ));
        }
    }

    None
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
    fn accepts_deprecated_with_argument_on_class_and_enum() {
        let attributes = [attribute(DEPRECATED_ATTRIBUTE, Some("reason"))];
        assert!(validate_attributes(&attributes, AttributeTarget::Class).is_empty());
        assert!(validate_attributes(&attributes, AttributeTarget::Enum).is_empty());
    }

    #[test]
    fn rejects_deprecated_without_argument_on_class_and_enum() {
        let attributes = [attribute(DEPRECATED_ATTRIBUTE, None)];
        let class_errors = validate_attributes(&attributes, AttributeTarget::Class);
        assert_eq!(class_errors.len(), 1);
        assert_eq!(class_errors[0].kind.properties().code, "E0114");

        let enum_errors = validate_attributes(&attributes, AttributeTarget::Enum);
        assert_eq!(enum_errors.len(), 1);
        assert_eq!(enum_errors[0].kind.properties().code, "E0114");
    }

    #[test]
    fn accepts_test_attribute_on_function() {
        let attributes = [attribute(TEST_ATTRIBUTE, None)];
        assert!(validate_attributes(&attributes, AttributeTarget::Function).is_empty());
    }

    #[test]
    fn rejects_ignore_without_test() {
        let attributes = [attribute(IGNORE_ATTRIBUTE, Some("reason"))];
        let errors = validate_attributes(&attributes, AttributeTarget::Function);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind.properties().code, "E0117");
    }

    #[test]
    fn rejects_xfail_without_test() {
        let attributes = [attribute(XFAIL_ATTRIBUTE, Some("reason"))];
        let errors = validate_attributes(&attributes, AttributeTarget::Function);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].kind.properties().code, "E0117");
    }

    #[test]
    fn accepts_test_with_ignore() {
        let attributes = [
            attribute(TEST_ATTRIBUTE, None),
            attribute(IGNORE_ATTRIBUTE, Some("reason")),
        ];
        assert!(validate_attributes(&attributes, AttributeTarget::Function).is_empty());
    }

    #[test]
    fn accepts_test_with_xfail() {
        let attributes = [
            attribute(TEST_ATTRIBUTE, None),
            attribute(XFAIL_ATTRIBUTE, Some("reason")),
        ];
        assert!(validate_attributes(&attributes, AttributeTarget::Function).is_empty());
    }

    #[test]
    fn accepts_test_with_ignore_and_xfail() {
        let attributes = [
            attribute(TEST_ATTRIBUTE, None),
            attribute(IGNORE_ATTRIBUTE, Some("reason1")),
            attribute(XFAIL_ATTRIBUTE, Some("reason2")),
        ];
        assert!(validate_attributes(&attributes, AttributeTarget::Function).is_empty());
    }
}
