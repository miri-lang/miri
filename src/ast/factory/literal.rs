// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::expr;
use super::primitives::literal;
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use crate::lexer::RegexToken;

/// Creates the smallest possible integer literal from an i128 value.
pub fn int(val: i128) -> IntegerLiteral {
    match val {
        v if v >= i8::MIN as i128 && v <= i8::MAX as i128 => IntegerLiteral::I8(v as i8),
        v if v >= i16::MIN as i128 && v <= i16::MAX as i128 => IntegerLiteral::I16(v as i16),
        v if v >= i32::MIN as i128 && v <= i32::MAX as i128 => IntegerLiteral::I32(v as i32),
        v if v >= i64::MIN as i128 && v <= i64::MAX as i128 => IntegerLiteral::I64(v as i64),
        _ => IntegerLiteral::I128(val),
    }
}

/// Creates an integer literal.
pub fn int_literal(val: i128) -> Literal {
    Literal::Integer(int(val))
}

/// Creates an integer literal expression.
pub fn int_literal_expression(val: i128) -> Expression {
    expr(ExpressionKind::Literal(int_literal(val)))
}

/// Creates a 32-bit float literal.
pub fn float32(val: f32) -> FloatLiteral {
    FloatLiteral::F32(val.to_bits())
}

/// Creates a 64-bit float literal.
pub fn float64(val: f64) -> FloatLiteral {
    FloatLiteral::F64(val.to_bits())
}

/// Creates a 32-bit float literal value.
pub fn float32_literal(val: f32) -> Literal {
    Literal::Float(float32(val))
}

/// Creates a 32-bit float literal expression.
pub fn float32_literal_expression(val: f32) -> Expression {
    literal(float32_literal(val))
}

/// Creates a 64-bit float literal value.
pub fn float64_literal(val: f64) -> Literal {
    Literal::Float(float64(val))
}

/// Creates a 64-bit float literal expression.
pub fn float64_literal_expression(val: f64) -> Expression {
    literal(float64_literal(val))
}

/// Creates a string literal.
pub fn string_literal(val: &str) -> Literal {
    Literal::String(val.to_string())
}

/// Creates a string literal expression.
pub fn string_literal_expression(val: &str) -> Expression {
    expr(ExpressionKind::Literal(string_literal(val)))
}

/// Creates an f-string expression (interpolated string).
pub fn f_string(parts: Vec<Expression>) -> Expression {
    expr(ExpressionKind::FormattedString(parts))
}

/// Creates a boolean literal.
pub fn boolean(val: bool) -> Literal {
    Literal::Boolean(val)
}

/// Creates a boolean literal expression.
pub fn boolean_literal(val: bool) -> Expression {
    expr(ExpressionKind::Literal(boolean(val)))
}

/// Creates an identifier literal (internal, used for function/type references in MIR).
pub fn identifier_literal_value(val: &str) -> Literal {
    Literal::Identifier(val.to_string())
}

/// Creates a regex literal from a token.
pub fn regex_literal_from_token(value: RegexToken) -> Literal {
    Literal::Regex(value)
}

/// Creates a regex literal expression from pattern and flags strings.
pub fn regex_literal(body: &str, flags: &str) -> Expression {
    let token = RegexToken {
        body: body.to_string(),
        ignore_case: flags.contains('i'),
        global: flags.contains('g'),
        multiline: flags.contains('m'),
        dot_all: flags.contains('s'),
        unicode: flags.contains('u'),
    };
    expr(ExpressionKind::Literal(regex_literal_from_token(token)))
}
