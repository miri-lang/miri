// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The enum `T?` spells, found by its attribute.
//!
//! `T?` is the enum a module declares `@optional`. The compiler learns which
//! enum that is, and which of its variants is the absent one and which
//! carries the payload, from the declaration's shape, never from its names.
//! The shape is checked where the enum is declared: one type parameter, one
//! variant with no payload, one carrying exactly that parameter, no drop
//! hook, no `equals` or `hash` of its own, and a closed variant set. A
//! second marked enum is refused, so `T?` names one type.
//!
//! The compiler keeps its own layout for the optional, a nullable word for a
//! counted payload, so a value of the enum is never laid out as an enum is:
//! the rules refuse every declaration that would observe the difference.

use crate::ast::statement::DROP_HOOK_NAME;
use crate::ast::types::{Type, TypeKind, EQUALS_METHOD_NAME, HASH_METHOD_NAME};
use crate::ast::OPTIONAL_ATTRIBUTE;
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::context::EnumDefinition;
use crate::type_checker::TypeChecker;

/// The enum `T?` spells, with its two variants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionalEnum {
    /// The enum's identity.
    pub name: String,
    /// The variant with no payload: the absent value.
    pub absent_variant: String,
    /// The variant carrying the payload.
    pub present_variant: String,
    /// The type parameter the present variant carries.
    pub parameter: String,
}

impl OptionalEnum {
    /// The optional holding `payload`: the enum's one spelling, `T?`, for its
    /// own `self` and for every instance of it.
    pub fn type_at(payload: Type) -> Type {
        let span = payload.span;
        Type::new(TypeKind::Option(Box::new(payload)), span)
    }

    /// The type a body compiled once for every payload holds its receiver
    /// as: the optional at its own type parameter.
    pub fn shared_self_type(&self, span: Span) -> Type {
        let parameter =
            TypeKind::Generic(self.parameter.clone(), crate::ast::bounds::Bounds::none());
        Self::type_at(Type::new(parameter, span))
    }
}

/// Why an enum marked `@optional` cannot be the optional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ShapeFault {
    /// It declares this many type parameters rather than one.
    TypeParameters(usize),
    /// It declares this many variants rather than two.
    Variants(usize),
    /// No variant is without a payload.
    NoAbsentVariant,
    /// No variant carries exactly the type parameter.
    NoPresentVariant,
    /// It declares a method the optional's own rules answer.
    OwnMethod(&'static str),
    /// Its variant set may grow.
    NonExhaustive,
}

impl ShapeFault {
    fn reason(&self) -> String {
        match self {
            ShapeFault::TypeParameters(count) => {
                format!("it declares {count} type parameters, and the optional declares one")
            }
            ShapeFault::Variants(count) => {
                format!("it declares {count} variants, and the optional declares two")
            }
            ShapeFault::NoAbsentVariant => {
                "none of its variants is without a payload, and the optional's absent value is one"
                    .to_string()
            }
            ShapeFault::NoPresentVariant => {
                "none of its variants carries exactly its type parameter, and the optional's \
                 present value is one"
                    .to_string()
            }
            ShapeFault::OwnMethod(method) => format!(
                "it declares `{method}`, and the optional's {method} is the compiler's, the \
                 same for every payload"
            ),
            ShapeFault::NonExhaustive => {
                "it is `@non_exhaustive`, and the optional's two variants are all it has"
                    .to_string()
            }
        }
    }
}

/// The optional `definition` declares, named `name`, or why its shape is
/// not the optional's.
pub(crate) fn optional_shape(
    name: &str,
    definition: &EnumDefinition,
) -> Result<OptionalEnum, ShapeFault> {
    if definition.non_exhaustive {
        return Err(ShapeFault::NonExhaustive);
    }
    let parameter = match definition.generics.as_deref() {
        Some([parameter]) => parameter.name.as_str(),
        Some(parameters) => return Err(ShapeFault::TypeParameters(parameters.len())),
        None => return Err(ShapeFault::TypeParameters(0)),
    };
    if definition.variants.len() != 2 {
        return Err(ShapeFault::Variants(definition.variants.len()));
    }
    if let Some(method) = [DROP_HOOK_NAME, EQUALS_METHOD_NAME, HASH_METHOD_NAME]
        .into_iter()
        .find(|method| definition.methods.contains_key(*method))
    {
        return Err(ShapeFault::OwnMethod(method));
    }
    let absent = (definition.variants.iter())
        .find(|(_, payload)| payload.is_empty())
        .ok_or(ShapeFault::NoAbsentVariant)?;
    let present = (definition.variants.iter())
        .find(|(_, payload)| {
            matches!(
                payload.as_slice(),
                [only] if matches!(&only.kind, TypeKind::Generic(name, _) if name == parameter)
            )
        })
        .ok_or(ShapeFault::NoPresentVariant)?;
    Ok(OptionalEnum {
        name: name.to_string(),
        absent_variant: absent.0.clone(),
        present_variant: present.0.clone(),
        parameter: parameter.to_string(),
    })
}

impl TypeChecker {
    /// The enum `T?` spells, once a module has declared it.
    pub fn optional_enum(&self) -> Option<&OptionalEnum> {
        self.type_table.optional.as_ref()
    }

    /// Whether the enum `name`, declared with `attributes`, is the one `T?`
    /// spells, before its shape is checked: it is marked, and no other enum
    /// is the optional yet. A marked enum of the wrong shape is refused, so
    /// reading it as the optional while its signatures resolve is never
    /// compiled.
    pub(crate) fn may_be_optional_enum(
        &self,
        name: &str,
        attributes: &[crate::ast::Attribute],
    ) -> bool {
        crate::type_checker::attributes::has_attribute(attributes, OPTIONAL_ATTRIBUTE)
            && self
                .optional_enum()
                .is_none_or(|optional| optional.name == name)
    }

    /// Takes the enum `name`, declared at `span` with `attributes`, as the
    /// optional when it is marked so: its shape is checked, and a second
    /// marked enum is refused. A type declared inside a function is refused
    /// on its own, and is never the optional.
    pub(crate) fn record_optional_enum(
        &mut self,
        (name, definition): (&str, &EnumDefinition),
        attributes: &[crate::ast::Attribute],
        (span, is_local): (Span, bool),
    ) {
        if is_local
            || !crate::type_checker::attributes::has_attribute(attributes, OPTIONAL_ATTRIBUTE)
        {
            return;
        }
        let optional = match optional_shape(name, definition) {
            Ok(optional) => optional,
            Err(fault) => return self.refuse_optional_shape(name, &fault, span),
        };
        match self.type_table.optional.as_ref() {
            Some(existing) if existing.name != optional.name => {
                let message = format!(
                    "'{name}' is marked `@optional`, and '{}' already is: `T?` names one type",
                    existing.name
                );
                let help = format!("remove `@optional` from '{name}'");
                self.report_error_with_help(
                    DiagnosticCode::TypOptionalDeclaration,
                    message,
                    span,
                    help,
                );
            }
            Some(_) | None => self.type_table.optional = Some(optional),
        }
    }

    fn refuse_optional_shape(&mut self, name: &str, fault: &ShapeFault, span: Span) {
        let message = format!("'{name}' is marked `@optional`, but {}", fault.reason());
        let help = "declare `@optional enum Name<T>` at the top level, with one variant without \
                    a payload and one carrying `T`, and no `drop`, `equals` or `hash`"
            .to_string();
        self.report_error_with_help(DiagnosticCode::TypOptionalDeclaration, message, span, help);
    }
}
