// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The one way a type error that compares two types is reported.
//!
//! Such an error says what was found, what was expected and how to fix it:
//! the message names both types as source spells them, the label sits on the
//! expression whose type is wrong, the help names the edit, and the diagnostic
//! carries both types as structured fields. Every code that compares types is
//! raised through [`TypeChecker::report_type_mismatch`] or one of its siblings
//! here, none of which can be called without both sides and a help.

use super::utils::MatchedBy;
use super::TypeChecker;
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{Type, TypeKind};
use crate::diagnostics::{DiagnosticCode, RepairRequest};
use crate::error::diagnostic::RelatedNote;
use crate::error::syntax::Span;
use crate::error::type_error::TypeError;
use crate::type_checker::diagnostics::spelled;

/// The two sides a type error compares, each spelled as source spells it.
pub(crate) struct TypeSides {
    pub expected: String,
    pub found: String,
}

impl TypeSides {
    /// A value of type `found` where one of type `expected` belongs.
    pub(crate) fn of(expected: &Type, found: &Type) -> Self {
        Self {
            expected: spelled(expected),
            found: spelled(found),
        }
    }

    /// A value of type `found` where no single type is expected, only one
    /// that `wanted` describes (`a boolean`, `a numeric type`).
    pub(crate) fn wanting(wanted: &str, found: &Type) -> Self {
        Self {
            expected: wanted.to_string(),
            found: spelled(found),
        }
    }
}

/// What a reported type mismatch carries beyond its code, message and sides.
#[derive(Default)]
pub(crate) struct MismatchExtras {
    pub repair: Option<RepairRequest>,
    pub related: Vec<RelatedNote>,
}

impl TypeChecker {
    /// Reports a type error that compares two types: `message` names both
    /// `sides`, `help` names the fix.
    pub(crate) fn report_type_mismatch(
        &mut self,
        code: DiagnosticCode,
        message: String,
        span: Span,
        sides: TypeSides,
        help: String,
    ) {
        self.push_type_mismatch(code, message, span, sides, help, MismatchExtras::default());
    }

    /// Records the mismatch once per (message, span), like every type error.
    pub(crate) fn push_type_mismatch(
        &mut self,
        code: DiagnosticCode,
        message: String,
        span: Span,
        sides: TypeSides,
        help: String,
        extras: MismatchExtras,
    ) {
        if self.suppress_diagnostics {
            return;
        }
        if !self.diagnostics.mark_reported((message.clone(), span)) {
            return;
        }
        let mut err = TypeError::coded_with_types(
            code,
            message,
            span,
            Some(help),
            Some(sides.expected),
            Some(sides.found),
        );
        err.source_override = self.modules.current_source_override.clone();
        err.repair = extras.repair;
        err.related = extras.related;
        self.diagnostics.push_error(err);
    }

    /// The fix for a value of type `found` where `expected` belongs, when the
    /// site knows nothing more specific to say.
    pub(crate) fn conversion_help(&self, expected: &Type, found: &Type) -> String {
        let (expected_text, found_text) = (spelled(expected), spelled(found));
        if self.is_numeric_type(&expected.kind) && self.is_numeric_type(&found.kind) {
            return format!(
                "convert the value with `as {expected_text}`, which truncates or rounds a value \
                 outside its range"
            );
        }
        if let TypeKind::Option(inner) = &found.kind {
            if spelled(inner) == expected_text {
                return format!(
                    "the value may be absent; handle `None` first (with `match`, `if let` or `??`) \
                     to get the `{expected_text}` inside"
                );
            }
        }
        format!(
            "use a value of type `{expected_text}` here, or change the type this place declares \
             to `{found_text}`"
        )
    }

    /// The cast [`Self::conversion_help`] names for a number of type `found`
    /// stored where `expected` belongs, as an edit on `value`: both must be
    /// numeric types, and `value` must be text a reader wrote.
    pub(crate) fn cast_repair(
        &self,
        expected: &Type,
        found: &Type,
        value: &Expression,
    ) -> Option<RepairRequest> {
        let numeric = self.is_numeric_type(&expected.kind) && self.is_numeric_type(&found.kind);
        (numeric && !value.span.is_empty()).then(|| RepairRequest::CastToSlotType {
            start: value.span.start,
            end: value.span.end,
            target: spelled(expected),
            parenthesize: !binds_tighter_than_a_cast(value),
        })
    }

    /// Refuses iterating over a value of type `ty`, which holds no elements.
    pub(crate) fn refuse_not_iterable(&mut self, ty: &Type, span: Span) {
        self.report_type_mismatch(
            DiagnosticCode::TypTypeMismatch,
            format!(
                "a `for` loop iterates over a range or a collection, but this is `{}`",
                spelled(ty)
            ),
            span,
            TypeSides::wanting("a range or a collection", ty),
            "iterate over a range such as `0..n`, or over a list, array, set or map".to_string(),
        );
    }

    /// Refuses an optional type `ty` as a Set element or a Map key: a
    /// collection matches those by value, and an absent value has none.
    pub(crate) fn refuse_optional_member(&mut self, member: MatchedBy, ty: &Type, span: Span) {
        let TypeKind::Option(inner) = &ty.kind else {
            return;
        };
        if matches!(inner.kind, TypeKind::Void) {
            self.report_type_mismatch(
                DiagnosticCode::TypCollectionElementType,
                format!("{} cannot hold only `None`", member.collection()),
                span,
                TypeSides::wanting("an element type", ty),
                format!(
                    "`None` alone has no element type; build an empty one instead, e.g. `{}`",
                    member.empty_example()
                ),
            );
            return;
        }
        let (role, help) = match member {
            MatchedBy::SetElement => (
                "a Set element",
                format!(
                    "store `{}` and leave the absent values out of the set",
                    spelled(inner)
                ),
            ),
            MatchedBy::MapKey => (
                "a Map key",
                format!(
                    "key the map by `{}` and leave the absent keys out",
                    spelled(inner)
                ),
            ),
        };
        self.report_type_mismatch(
            DiagnosticCode::TypCollectionElementType,
            format!(
                "{role} cannot be optional, but this one is `{}`",
                spelled(ty)
            ),
            span,
            TypeSides::of(inner, ty),
            help,
        );
    }

    /// The fix for a literal element of type `found` among elements of type
    /// `first`.
    pub(crate) fn literal_element_help(&self, first: &Type, found: &Type) -> String {
        if self.is_numeric_type(&first.kind) && self.is_numeric_type(&found.kind) {
            return format!("convert this element with `as {}`", spelled(first));
        }
        format!(
            "write every element as `{}`, or declare the collection's element type",
            spelled(first)
        )
    }

    /// Refuses the condition of an `if` or `while` (`construct`) whose type
    /// `ty` is not `bool`.
    pub(crate) fn refuse_non_boolean_condition(&mut self, construct: &str, ty: &Type, span: Span) {
        let help = if matches!(ty.kind, TypeKind::Option(_)) {
            "test whether it holds a value with `if let Some(v) = ...`, or compare it with `None`"
                .to_string()
        } else if self.is_numeric_type(&ty.kind) {
            "write the comparison that yields a `bool`, e.g. `n != 0`".to_string()
        } else {
            "write an expression that yields a `bool`, such as a comparison".to_string()
        };
        self.report_type_mismatch(
            DiagnosticCode::TypTypeMismatch,
            format!(
                "{construct} condition must be a boolean, got {}",
                spelled(ty)
            ),
            span,
            TypeSides::of(&crate::ast::factory::make_type(TypeKind::Boolean), ty),
            help,
        );
    }

    /// Refuses a loop variable declared `declared` over elements of type
    /// `element`.
    pub(crate) fn refuse_loop_variable_type(
        &mut self,
        decl: &crate::ast::statement::VariableDeclaration,
        declared: &Type,
        element: &Type,
        span: Span,
    ) {
        self.report_type_mismatch(
            DiagnosticCode::TypTypeMismatch,
            format!(
                "Type mismatch for loop variable '{}': the loop yields `{}`, but it is declared `{}`",
                decl.name,
                spelled(element),
                spelled(declared)
            ),
            span,
            TypeSides::of(element, declared),
            format!(
                "declare `{}` as `{}`, or leave its type out to take the type the loop yields",
                decl.name,
                spelled(element)
            ),
        );
    }

    /// Refuses a returned value of type `found` from a function declared to
    /// return `expected`.
    pub(crate) fn refuse_return_type(&mut self, expected: &Type, found: &Type, span: Span) {
        let help = if self.is_numeric_type(&expected.kind) && self.is_numeric_type(&found.kind) {
            format!("convert the value with `as {}`", spelled(expected))
        } else {
            format!(
                "return a `{}`, or declare the function's return type as `{}`",
                spelled(expected),
                spelled(found)
            )
        };
        self.report_type_mismatch(
            DiagnosticCode::TypTypeMismatch,
            format!(
                "Invalid return type: expected {}, got {}",
                spelled(expected),
                spelled(found)
            ),
            span,
            TypeSides::of(expected, found),
            help,
        );
    }
}

/// Whether `value` reads as one operand to the `as` written after it, so the
/// cast converts all of it without parentheses: a name, a literal, a call, a
/// member or an index. Anything with an operator of its own is wrapped.
fn binds_tighter_than_a_cast(value: &Expression) -> bool {
    match &value.node {
        ExpressionKind::Identifier(..)
        | ExpressionKind::Call(..)
        | ExpressionKind::Member(..)
        | ExpressionKind::Index(..)
        | ExpressionKind::Literal(..) => true,
        ExpressionKind::Binary(..)
        | ExpressionKind::Logical(..)
        | ExpressionKind::Unary(..)
        | ExpressionKind::Assignment(..)
        | ExpressionKind::Conditional(..)
        | ExpressionKind::Range(..)
        | ExpressionKind::Guard(..)
        | ExpressionKind::ImportPath(..)
        | ExpressionKind::Type(..)
        | ExpressionKind::GenericType(..)
        | ExpressionKind::TypeDeclaration(..)
        | ExpressionKind::EnumValue(..)
        | ExpressionKind::StructMember(..)
        | ExpressionKind::Lambda(..)
        | ExpressionKind::List(..)
        | ExpressionKind::Array(..)
        | ExpressionKind::Map(..)
        | ExpressionKind::Tuple(..)
        | ExpressionKind::Set(..)
        | ExpressionKind::Match(..)
        | ExpressionKind::FormattedString(..)
        | ExpressionKind::NamedArgument(..)
        | ExpressionKind::Super
        | ExpressionKind::Block(..)
        | ExpressionKind::Cast(..) => false,
    }
}
