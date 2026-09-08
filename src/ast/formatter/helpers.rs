// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Shared renderings used by both the statement and expression formatters:
//! operators, literals, parameter lists, and declaration modifiers.

use crate::ast::common::{FunctionProperties, MemberVisibility, Parameter};
use crate::ast::expression::ExpressionKind;
use crate::ast::literal::{FloatLiteral, Literal};
use crate::ast::operator::{AssignmentOp, BinaryOp, GuardOp, UnaryOp};
use crate::ast::statement::BindingResidency;
use crate::ast::types::TypeKind;
use crate::error::syntax::Span;
use crate::lexer::RegexToken;

use super::expression::expression as format_expression;
use super::sink::Sink;

/// The source spelling of a binary operator.
pub fn binary_operator(operator: BinaryOp) -> &'static str {
    match operator {
        BinaryOp::Add => "+",
        BinaryOp::Sub => "-",
        BinaryOp::Mul => "*",
        BinaryOp::Div => "/",
        BinaryOp::Mod => "%",
        BinaryOp::BitwiseOr => "|",
        BinaryOp::BitwiseAnd => "&",
        BinaryOp::BitwiseXor => "^",
        BinaryOp::Equal => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::LessThan => "<",
        BinaryOp::LessThanEqual => "<=",
        BinaryOp::GreaterThan => ">",
        BinaryOp::GreaterThanEqual => ">=",
        BinaryOp::Not => "not",
        BinaryOp::And => "and",
        BinaryOp::Or => "or",
        BinaryOp::Range => "..",
        BinaryOp::In => "in",
        BinaryOp::NullCoalesce => "??",
    }
}

/// The source spelling of a guard operator.
pub fn guard_operator(operator: GuardOp) -> &'static str {
    match operator {
        GuardOp::NotEqual => "!=",
        GuardOp::LessThan => "<",
        GuardOp::LessThanEqual => "<=",
        GuardOp::GreaterThan => ">",
        GuardOp::GreaterThanEqual => ">=",
        GuardOp::Not => "not",
        GuardOp::NotIn => "not in",
        GuardOp::In => "in",
    }
}

/// The source spelling of an assignment operator.
pub fn assignment_operator(operator: AssignmentOp) -> &'static str {
    match operator {
        AssignmentOp::Assign => "=",
        AssignmentOp::AssignAdd => "+=",
        AssignmentOp::AssignSub => "-=",
        AssignmentOp::AssignMul => "*=",
        AssignmentOp::AssignDiv => "/=",
        AssignmentOp::AssignMod => "%=",
    }
}

/// The source spelling of a unary operator.
///
/// `await` and `not` carry a trailing space because they are words rather
/// than symbols, and `not x` would otherwise render as `notx`. There is no
/// `!` operator in the language, so negation only ever spells out.
pub fn unary_operator(operator: UnaryOp) -> &'static str {
    match operator {
        UnaryOp::Negate => "-",
        UnaryOp::Not => "not ",
        UnaryOp::Plus => "+",
        UnaryOp::BitwiseNot => "~",
        UnaryOp::Increment => "++",
        UnaryOp::Decrement => "--",
        UnaryOp::Await => "await ",
    }
}

/// Render a literal in source syntax.
pub fn literal(sink: &mut Sink, value: &Literal, span: Span) {
    match value {
        Literal::Integer(integer) => match numeric_spelling(sink, value, span) {
            Some(text) => sink.emit(&text),
            None => sink.emit(&integer.to_string()),
        },
        Literal::Float(float) => match numeric_spelling(sink, value, span) {
            Some(text) => sink.emit(&text),
            None => sink.emit(&float_text(float)),
        },
        Literal::String(text) => {
            sink.emit("\"");
            sink.emit(&escape_string(text));
            sink.emit("\"");
        }
        Literal::Boolean(true) => sink.emit("true"),
        Literal::Boolean(false) => sink.emit("false"),
        Literal::Identifier(name) => sink.emit(name),
        Literal::Regex(regex) => regex_literal(sink, regex),
        Literal::None => sink.emit("None"),
    }
}

/// The spelling the source gave this number, when the span holds one and it
/// means the same number.
///
/// A number can be written many ways — `0xFF`, `255`, `1_000`, `2e5` — and the
/// tree keeps only the value, so rendering from the tree alone silently picks
/// one of them and discards the author's. The source is consulted instead.
///
/// The answer is used only when reading the text back gives this very value,
/// so a span that does not cover exactly this number cannot be pasted into the
/// output: the check fails and the tree's own rendering is used, which is what
/// rendering did before the source was consulted at all.
fn numeric_spelling(sink: &Sink, value: &Literal, span: Span) -> Option<String> {
    let text = sink.source_text(span)?;
    if !text.starts_with(|first: char| first.is_ascii_digit()) {
        return None;
    }
    if !text
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '+' || c == '-')
    {
        return None;
    }
    let means_the_same = match value {
        Literal::Integer(integer) => written_integer(text) == Some(integer.to_i128()),
        Literal::Float(float) => written_float(text, float),
        Literal::String(_)
        | Literal::Boolean(_)
        | Literal::Identifier(_)
        | Literal::Regex(_)
        | Literal::None => false,
    };
    means_the_same.then(|| text.to_string())
}

/// The value `text` denotes when it is an integer literal.
fn written_integer(text: &str) -> Option<i128> {
    let digits: String = text.chars().filter(|c| *c != '_').collect();
    let (body, radix) = match digits.get(..2) {
        Some("0b") | Some("0B") => (&digits[2..], 2),
        Some("0o") | Some("0O") => (&digits[2..], 8),
        Some("0x") | Some("0X") => (&digits[2..], 16),
        _ => (digits.as_str(), 10),
    };
    i128::from_str_radix(body, radix).ok()
}

/// Whether `text` is a float literal denoting exactly `value`.
fn written_float(text: &str, value: &FloatLiteral) -> bool {
    let digits: String = text.chars().filter(|c| *c != '_').collect();
    match value {
        FloatLiteral::F32(bits) => digits.parse::<f32>() == Ok(f32::from_bits(*bits)),
        FloatLiteral::F64(bits) => digits.parse::<f64>() == Ok(f64::from_bits(*bits)),
    }
}

/// Render a regex literal with the flags it was written with.
fn regex_literal(sink: &mut Sink, regex: &RegexToken) {
    sink.emit("re\"");
    sink.emit(&regex.body);
    sink.emit("\"");
    if regex.ignore_case {
        sink.emit("i");
    }
    if regex.global {
        sink.emit("g");
    }
    if regex.multiline {
        sink.emit("m");
    }
    if regex.dot_all {
        sink.emit("s");
    }
    if regex.unicode {
        sink.emit("u");
    }
}

/// A literal that overflows to infinity in either float width.
///
/// Infinity has no finite decimal spelling, but it is reachable as a literal:
/// the stdlib writes `1e309`, which overflows on the way in. Rendering that
/// same overflow back is what lets such a constant survive a round trip.
const OVERFLOWING_LITERAL: &str = "1e309";

/// Render a float so it lexes back to the same value.
///
/// The literal's own `Display` prints `1.0` as `1`, which re-reads as an
/// integer and changes the parsed type, so a decimal point is forced whenever
/// the shortest round-trip form does not already carry one. A non-finite value
/// needs more than that: `{:?}` prints infinity as `inf`, and `inf.0` re-reads
/// as a member access rather than a number.
pub fn float_text(value: &FloatLiteral) -> String {
    let (text, number) = match value {
        FloatLiteral::F32(bits) => {
            let number = f32::from_bits(*bits);
            (format!("{:?}", number), number as f64)
        }
        FloatLiteral::F64(bits) => {
            let number = f64::from_bits(*bits);
            (format!("{:?}", number), number)
        }
    };
    if !number.is_finite() {
        return non_finite_text(number);
    }
    if text.contains('.') {
        return text;
    }
    match text.split_once(['e', 'E']) {
        // `1e10` has no decimal point of its own; give the mantissa one.
        Some((mantissa, exponent)) => format!("{}.0e{}", mantissa, exponent),
        None => format!("{}.0", text),
    }
}

/// Render a value that is not a finite number.
///
/// A NaN literal cannot be written in Miri, so it is unreachable from parsed
/// source; it renders as an overflowing literal rather than as a word no lexer
/// would accept.
fn non_finite_text(number: f64) -> String {
    if number == f64::NEG_INFINITY {
        return format!("-{}", OVERFLOWING_LITERAL);
    }
    OVERFLOWING_LITERAL.to_string()
}

/// Escape a string so it re-lexes to the same contents.
pub fn escape_string(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            other => escaped.push(other),
        }
    }
    escaped
}

/// Render a parenthesised parameter list.
pub fn parameter_list(sink: &mut Sink, parameters: &[Parameter]) {
    sink.emit("(");
    for (index, parameter) in parameters.iter().enumerate() {
        if index > 0 {
            sink.emit(", ");
        }
        parameter_declaration(sink, parameter);
    }
    sink.emit(")");
}

/// Render one parameter: `name [out] [residency] Type [guard] [= default]`.
///
/// Everything before the type is optional: a parameter in a function type is
/// written as its type alone (`fn(int) int`), so the space that separates a
/// name from a type is written only where there is a name to separate.
///
/// `self` is the other case with nothing before its type — it names the
/// enclosing type by being `self`, and the parser supplies `Self` for it — so
/// it is written back the way it is written by hand, without one.
fn parameter_declaration(sink: &mut Sink, parameter: &Parameter) {
    let mut prefix: Vec<&str> = Vec::new();
    if !parameter.name.is_empty() {
        prefix.push(&parameter.name);
    }
    if parameter.is_out {
        prefix.push("out");
    }
    match parameter.residency {
        Some(BindingResidency::Gpu) => prefix.push("gpu"),
        Some(BindingResidency::Host) => prefix.push("host"),
        None => {}
    }
    for (index, part) in prefix.iter().enumerate() {
        if index > 0 {
            sink.emit(" ");
        }
        sink.emit(part);
    }
    if !names_its_own_type(parameter) {
        if !prefix.is_empty() {
            sink.emit(" ");
        }
        format_expression(sink, &parameter.typ, 0);
    }
    if let Some(guard) = &parameter.guard {
        sink.emit(" ");
        format_expression(sink, guard, 0);
    }
    if let Some(default) = &parameter.default_value {
        sink.emit(" = ");
        format_expression(sink, default, 0);
    }
}

/// Whether `parameter` is the `self` receiver, whose type the parser supplies.
///
/// `fn drop(self)` is the only way to write it: `self` names the enclosing type
/// by being `self`, and the parser fills in `Self` behind it. Writing that
/// `Self` back would add a word to a declaration that never had one.
fn names_its_own_type(parameter: &Parameter) -> bool {
    if parameter.name != "self" {
        return false;
    }
    let ExpressionKind::Type(declared, false) = &parameter.typ.node else {
        return false;
    };
    matches!(&declared.kind, TypeKind::Custom(name, None) if name == "Self")
}

/// Render a visibility modifier and its trailing space, if it is not the default.
pub fn visibility(sink: &mut Sink, level: &MemberVisibility) {
    match level {
        // Public is the default, so the keyword is written back only where the
        // source wrote it. Emitting it always would add a modifier to every
        // declaration in every file the formatter touches; emitting it never
        // deletes one from every declaration that had it.
        MemberVisibility::Public => {
            if sink.written_modifiers().public {
                sink.emit("public ");
            }
        }
        MemberVisibility::Protected => sink.emit("protected "),
        MemberVisibility::Private => sink.emit("private "),
    }
}

/// Render the modifiers that precede `fn` on a function declaration.
pub fn function_modifiers(sink: &mut Sink, properties: &FunctionProperties) {
    visibility(sink, &properties.visibility);
    // A method with no body is abstract whether or not the keyword is there,
    // so this is written back only where the source wrote it — the same rule
    // `public` follows, and for the same reason.
    if sink.written_modifiers().is_abstract {
        sink.emit("abstract ");
    }
    if properties.is_static {
        sink.emit("static ");
    }
    if properties.is_async {
        sink.emit("async ");
    }
    if properties.is_parallel {
        sink.emit("parallel ");
    }
    if properties.is_gpu {
        sink.emit("gpu ");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escape_string_escapes_newline() {
        assert_eq!(escape_string("hello\nworld"), "hello\\nworld");
    }

    #[test]
    fn test_escape_string_escapes_quote_and_backslash() {
        assert_eq!(escape_string("say \"hi\""), "say \\\"hi\\\"");
        assert_eq!(escape_string("a\\b"), "a\\\\b");
    }

    #[test]
    fn test_float_text_keeps_a_decimal_point() {
        assert_eq!(float_text(&FloatLiteral::F64(1.0f64.to_bits())), "1.0");
        assert_eq!(float_text(&FloatLiteral::F64(1.5f64.to_bits())), "1.5");
    }

    #[test]
    fn test_float_text_renders_infinity_as_an_overflowing_literal() {
        // `{:?}` prints infinity as `inf`, and `inf.0` re-reads as a member
        // access. The stdlib reaches infinity by writing `1e309`.
        let rendered = float_text(&FloatLiteral::F64(f64::INFINITY.to_bits()));
        assert_eq!(rendered, "1e309");
        assert!(rendered
            .parse::<f64>()
            .is_ok_and(|value| value.is_infinite()));
    }

    #[test]
    fn test_float_text_gives_an_exponent_form_a_decimal_point() {
        let rendered = float_text(&FloatLiteral::F64(1e300f64.to_bits()));
        assert!(
            rendered.contains('.'),
            "expected a decimal point: {rendered}"
        );
    }
}
