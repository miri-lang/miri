// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Format specs checked against the values they format.

use crate::ast::format_spec::{self, FormatSpec, ParsedSpec, Presentation, SpecError};
use crate::ast::types::{Type, TypeKind};
use crate::ast::Expression;
use crate::diagnostics::DiagnosticCode;
use crate::type_checker::diagnostics::spelled;
use crate::type_checker::TypeChecker;

/// The help for a spec Miri cannot read: its grammar and three examples.
const UNREADABLE_HELP: &str = "write the spec as [[fill]align][0][width][.precision][type]: \
     {x:.2f} rounds to two decimals, {n:>8} right-aligns in eight columns, {n:08x} writes \
     zero-padded hex";

/// The help for a spec past the limits [`format_spec::MAX_WIDTH`] and
/// [`format_spec::MAX_PRECISION`] set.
const TOO_LARGE_HELP: &str = "a width is at most 1000 and a precision at most 100";

/// What kind of value a spec formats, as far as a spec cares.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ValueClass {
    Float,
    /// An integer of up to 64 bits.
    Integer,
    /// A 128-bit integer, which formats in decimal only.
    WideInteger,
    Other,
}

/// Why a readable spec cannot format a value: the form it asks for, what that
/// form applies to, and the help.
struct Misfit {
    form: String,
    help: String,
}

impl TypeChecker {
    /// Refuses each spec in `specs` that cannot format the part it follows.
    /// The parts are already inferred, so their types are recorded.
    pub(crate) fn check_format_specs(
        &mut self,
        parts: &[Expression],
        specs: &[(usize, FormatSpec)],
    ) {
        for (index, spec) in specs {
            let Some(ty) = parts
                .get(*index)
                .and_then(|part| self.get_type(part.id))
                .cloned()
            else {
                continue;
            };
            if crate::ast::types::is_unresolved(&ty.kind) {
                continue;
            }
            if let Some((message, help)) = refusal(&spec.text, &ty) {
                self.report_error_with_help(
                    DiagnosticCode::TypInvalidFormatSpec,
                    message,
                    spec.span,
                    help,
                );
            }
        }
    }
}

/// The message and help refusing `text` as a spec for a value of type `ty`,
/// or `None` when it formats that value.
fn refusal(text: &str, ty: &Type) -> Option<(String, String)> {
    let parsed = match format_spec::parse(text) {
        Ok(parsed) => parsed,
        Err(error) => return Some(unreadable(text, error)),
    };
    let misfit = misfit(&parsed, value_class(&ty.kind))?;
    Some((
        format!(
            "format spec '{text}' writes {}, but the value is '{}'",
            misfit.form,
            spelled(ty)
        ),
        misfit.help,
    ))
}

fn unreadable(text: &str, error: SpecError) -> (String, String) {
    match error {
        SpecError::Unreadable => (
            format!("format spec '{text}' is not one Miri reads"),
            UNREADABLE_HELP.to_string(),
        ),
        SpecError::PrecisionWithoutType(digits) => (
            format!("format spec '{text}' gives a precision without a type"),
            format!("write `.{digits}f` for {digits} digits after the point"),
        ),
        SpecError::TooLarge => (
            format!(
                "format spec '{text}' asks for a wider field or more decimals than Miri writes"
            ),
            TOO_LARGE_HELP.to_string(),
        ),
    }
}

/// What in `spec` a value of `class` cannot be written as.
fn misfit(spec: &ParsedSpec, class: ValueClass) -> Option<Misfit> {
    if let Some(misfit) = spec
        .presentation
        .and_then(|presentation| presentation_misfit(presentation, class))
    {
        return Some(misfit);
    }
    let numeric = matches!(
        class,
        ValueClass::Float | ValueClass::Integer | ValueClass::WideInteger
    );
    (spec.zero && !numeric).then(|| applies_to("a zero-padded number", "`0`", "numbers"))
}

/// Why `presentation` cannot write a value of `class`.
fn presentation_misfit(presentation: Presentation, class: ValueClass) -> Option<Misfit> {
    match presentation {
        Presentation::Fixed => {
            (!matches!(class, ValueClass::Float | ValueClass::Integer)).then(|| {
                applies_to(
                    "a fixed-point number",
                    "`f`",
                    "floats and integers of up to 64 bits",
                )
            })
        }
        Presentation::Decimal
        | Presentation::Hex
        | Presentation::UpperHex
        | Presentation::Binary
        | Presentation::Octal => match class {
            ValueClass::Integer => None,
            ValueClass::Float => Some(Misfit {
                form: radix_form(presentation),
                help: "convert the value with `as int` first; a radix writes an integer".into(),
            }),
            ValueClass::WideInteger | ValueClass::Other => Some(applies_to(
                &radix_form(presentation),
                &format!("`{}`", radix_letter(presentation)),
                "integers of up to 64 bits",
            )),
        },
    }
}

fn applies_to(form: &str, flag: &str, applies: &str) -> Misfit {
    Misfit {
        form: form.to_string(),
        help: format!("{flag} applies to {applies}; drop it from the spec"),
    }
}

fn radix_form(radix: Presentation) -> String {
    match radix.radix() {
        Some(10) | None => "an integer in decimal".to_string(),
        Some(base) => format!("an integer in base {base}"),
    }
}

fn radix_letter(radix: Presentation) -> char {
    match radix {
        Presentation::Fixed => 'f',
        Presentation::Decimal => 'd',
        Presentation::Hex => 'x',
        Presentation::UpperHex => 'X',
        Presentation::Binary => 'b',
        Presentation::Octal => 'o',
    }
}

fn value_class(kind: &TypeKind) -> ValueClass {
    let float = matches!(
        kind,
        TypeKind::Float | TypeKind::F16 | TypeKind::F32 | TypeKind::F64
    );
    let integer = matches!(
        kind,
        TypeKind::Int
            | TypeKind::I8
            | TypeKind::I16
            | TypeKind::I32
            | TypeKind::I64
            | TypeKind::U8
            | TypeKind::U16
            | TypeKind::U32
            | TypeKind::U64
    );
    let wide = matches!(kind, TypeKind::I128 | TypeKind::U128);
    match (float, integer, wide) {
        (true, _, _) => ValueClass::Float,
        (_, true, _) => ValueClass::Integer,
        (_, _, true) => ValueClass::WideInteger,
        (false, false, false) => ValueClass::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_helps_state_the_limits_and_grammar_the_parser_uses() {
        assert!(UNREADABLE_HELP.contains(format_spec::SPEC_SHAPE));
        assert!(TOO_LARGE_HELP.contains(&format!("at most {} ", format_spec::MAX_WIDTH)));
        assert!(TOO_LARGE_HELP.ends_with(&format!("at most {}", format_spec::MAX_PRECISION)));
    }
}
