// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Renders expressions back to Miri source syntax.
//!
//! Parentheses are added from a precedence table rather than around every
//! composite operand. The table mirrors the layering of the expression rules in
//! the published grammar, so an operand is wrapped exactly when re-parsing the
//! unwrapped form would build a different tree.

use crate::ast::expression::{
    Expression, ExpressionKind, ImportPathKind, LambdaData, LeftHandSideExpression,
    RangeExpressionType,
};
use crate::ast::operator::BinaryOp;
use crate::ast::statement::IfStatementType;

use super::helpers::{
    assignment_operator, binary_operator, function_modifiers, guard_operator, is_postfix_unary,
    literal, parameter_list, unary_operator,
};
use super::sink::Sink;
use super::statement::statement as format_statement;
use super::types::{generic_arguments, type_expression};

/// Binding strength of an expression form, low to high. The values mirror the
/// order of the expression rules in the grammar; a larger number binds tighter.
mod precedence {
    /// A lambda body extends as far as it can, so a lambda is wrapped wherever
    /// it appears as an operand.
    pub const LAMBDA: u8 = 0;
    pub const ASSIGNMENT: u8 = 1;
    pub const CONDITIONAL: u8 = 2;
    pub const NULL_COALESCE: u8 = 3;
    /// Only the `or` keyword; `|` binds at [`ADDITIVE`].
    pub const LOGICAL_OR: u8 = 4;
    /// Only the `and` keyword; `&` binds at [`ADDITIVE`].
    pub const LOGICAL_AND: u8 = 5;
    pub const EQUALITY: u8 = 6;
    pub const RELATIONAL: u8 = 7;
    pub const RANGE: u8 = 8;
    pub const ADDITIVE: u8 = 9;
    pub const MULTIPLICATIVE: u8 = 10;
    pub const UNARY: u8 = 11;
    pub const POSTFIX: u8 = 12;
    pub const PRIMARY: u8 = 13;
}

/// Render an expression at the top of its own precedence context.
pub fn expression(sink: &mut Sink, expr: &Expression, indent: usize) {
    render(sink, expr, indent, precedence::LAMBDA);
}

/// Render `expr`, wrapping it in parentheses when it binds more loosely than
/// `minimum`.
fn render(sink: &mut Sink, expr: &Expression, indent: usize, minimum: u8) {
    let needs_parentheses = precedence_of(&expr.node) < minimum;
    if needs_parentheses {
        sink.emit("(");
    }
    kind(sink, &expr.node, indent);
    if needs_parentheses {
        sink.emit(")");
    }
}

/// The binding strength of one expression form.
fn precedence_of(kind: &ExpressionKind) -> u8 {
    match kind {
        ExpressionKind::Assignment(..) => precedence::ASSIGNMENT,
        ExpressionKind::Conditional(..) => precedence::CONDITIONAL,
        ExpressionKind::Binary(_, operator, _) | ExpressionKind::Logical(_, operator, _) => {
            binary_precedence(*operator)
        }
        ExpressionKind::Range(..) => precedence::RANGE,
        ExpressionKind::Guard(..) => precedence::RELATIONAL,
        ExpressionKind::Unary(operator, _) => {
            if is_postfix_unary(*operator) {
                precedence::POSTFIX
            } else {
                precedence::UNARY
            }
        }
        ExpressionKind::Member(..)
        | ExpressionKind::Index(..)
        | ExpressionKind::Call(..)
        | ExpressionKind::Cast(..)
        | ExpressionKind::EnumValue(..) => precedence::POSTFIX,
        ExpressionKind::Lambda(..) => precedence::LAMBDA,
        ExpressionKind::Literal(..)
        | ExpressionKind::Identifier(..)
        | ExpressionKind::ImportPath(..)
        | ExpressionKind::Type(..)
        | ExpressionKind::GenericType(..)
        | ExpressionKind::TypeDeclaration(..)
        | ExpressionKind::StructMember(..)
        | ExpressionKind::List(..)
        | ExpressionKind::Array(..)
        | ExpressionKind::Map(..)
        | ExpressionKind::Tuple(..)
        | ExpressionKind::Set(..)
        | ExpressionKind::Match(..)
        | ExpressionKind::FormattedString(..)
        | ExpressionKind::NamedArgument(..)
        | ExpressionKind::Super
        | ExpressionKind::Block(..) => precedence::PRIMARY,
    }
}

/// The binding strength of a binary operator.
fn binary_precedence(operator: BinaryOp) -> u8 {
    match operator {
        BinaryOp::NullCoalesce => precedence::NULL_COALESCE,
        BinaryOp::Or => precedence::LOGICAL_OR,
        BinaryOp::And => precedence::LOGICAL_AND,
        BinaryOp::Equal | BinaryOp::NotEqual => precedence::EQUALITY,
        BinaryOp::LessThan
        | BinaryOp::LessThanEqual
        | BinaryOp::GreaterThan
        | BinaryOp::GreaterThanEqual
        | BinaryOp::In
        | BinaryOp::Not => precedence::RELATIONAL,
        BinaryOp::Range => precedence::RANGE,
        // `|`, `&` and `^` bind at the additive level, with `+` and `-`: the
        // parser reads all five through one predicate (`is_additive_op`), and
        // only the `and` / `or` keywords sit at the logical levels.
        BinaryOp::Add
        | BinaryOp::Sub
        | BinaryOp::BitwiseOr
        | BinaryOp::BitwiseAnd
        | BinaryOp::BitwiseXor => precedence::ADDITIVE,
        BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => precedence::MULTIPLICATIVE,
    }
}

/// Render one expression form.
fn kind(sink: &mut Sink, node: &ExpressionKind, indent: usize) {
    match node {
        ExpressionKind::Literal(value) => literal(sink, value),
        ExpressionKind::Identifier(name, qualifier) => identifier(sink, name, qualifier.as_deref()),
        ExpressionKind::Binary(left, operator, right)
        | ExpressionKind::Logical(left, operator, right) => {
            binary(sink, left, *operator, right, indent)
        }
        ExpressionKind::Unary(operator, operand) => unary(sink, *operator, operand, indent),
        ExpressionKind::Assignment(target, operator, value) => {
            assignment(sink, target, *operator, value, indent)
        }
        ExpressionKind::Conditional(value, condition, alternative, form) => {
            conditional(sink, value, condition, alternative.as_deref(), form, indent)
        }
        ExpressionKind::Range(start, end, form) => range(sink, start, end.as_deref(), form, indent),
        ExpressionKind::Guard(operator, bound) => guard(sink, *operator, bound, indent),
        ExpressionKind::Member(object, member) => member_access(sink, object, member, indent),
        ExpressionKind::Index(object, index) => index_access(sink, object, index, indent),
        ExpressionKind::Call(callee, arguments) => call(sink, callee, arguments, indent),
        ExpressionKind::ImportPath(segments, form) => import_path(sink, segments, form, indent),
        ExpressionKind::Type(declared, is_nullable) => {
            type_expression(sink, declared, *is_nullable)
        }
        ExpressionKind::GenericType(name, arguments, _) => {
            generic_type(sink, name, arguments.as_deref(), indent)
        }
        ExpressionKind::TypeDeclaration(name, generics, declaration, bound) => type_declaration(
            sink,
            name,
            generics.as_deref(),
            declaration,
            bound.as_deref(),
            indent,
        ),
        ExpressionKind::EnumValue(path, payload) => enum_value(sink, path, payload, indent),
        ExpressionKind::StructMember(name, declared) => struct_member(sink, name, declared, indent),
        ExpressionKind::Lambda(data) => lambda(sink, data, indent),
        ExpressionKind::List(elements) => sequence(sink, "[", elements, "]", indent),
        ExpressionKind::Array(elements, _) => sequence(sink, "[", elements, "]", indent),
        ExpressionKind::Map(entries) => map(sink, entries, indent),
        ExpressionKind::Tuple(elements) => sequence(sink, "(", elements, ")", indent),
        ExpressionKind::Set(elements) => sequence(sink, "{", elements, "}", indent),
        ExpressionKind::Match(subject, branches) => {
            super::statement::match_expression(sink, subject, branches, indent)
        }
        ExpressionKind::FormattedString(parts) => formatted_string(sink, parts, indent),
        ExpressionKind::NamedArgument(name, value) => named_argument(sink, name, value, indent),
        ExpressionKind::Super => sink.emit("super"),
        ExpressionKind::Block(statements, value) => block(sink, statements, value, indent),
        ExpressionKind::Cast(value, target) => cast(sink, value, target, indent),
    }
}

/// `target op value`
fn assignment(
    sink: &mut Sink,
    target: &LeftHandSideExpression,
    operator: crate::ast::operator::AssignmentOp,
    value: &Expression,
    indent: usize,
) {
    assignment_target(sink, target, indent);
    sink.emit(" ");
    sink.emit(assignment_operator(operator));
    sink.emit(" ");
    render(sink, value, indent, precedence::ASSIGNMENT);
}

/// A parameter guard such as `> 0`.
fn guard(
    sink: &mut Sink,
    operator: crate::ast::operator::GuardOp,
    bound: &Expression,
    indent: usize,
) {
    sink.emit(guard_operator(operator));
    sink.emit(" ");
    render(sink, bound, indent, precedence::RANGE);
}

/// `object.member`
fn member_access(sink: &mut Sink, object: &Expression, member: &Expression, indent: usize) {
    render(sink, object, indent, precedence::POSTFIX);
    sink.emit(".");
    render(sink, member, indent, precedence::PRIMARY);
}

/// `object[index]`
fn index_access(sink: &mut Sink, object: &Expression, index: &Expression, indent: usize) {
    render(sink, object, indent, precedence::POSTFIX);
    sink.emit("[");
    expression(sink, index, indent);
    sink.emit("]");
}

/// `callee(arguments)`
fn call(sink: &mut Sink, callee: &Expression, arguments: &[Expression], indent: usize) {
    render(sink, callee, indent, precedence::POSTFIX);
    argument_list(sink, arguments, indent);
}

/// `Name<Argument>`
fn generic_type(sink: &mut Sink, name: &Expression, arguments: Option<&Expression>, indent: usize) {
    render(sink, name, indent, precedence::PRIMARY);
    let Some(arguments) = arguments else {
        return;
    };
    sink.emit("<");
    expression(sink, arguments, indent);
    sink.emit(">");
}

/// `Enum.Variant(payload)`
fn enum_value(sink: &mut Sink, path: &Expression, payload: &[Expression], indent: usize) {
    render(sink, path, indent, precedence::POSTFIX);
    if !payload.is_empty() {
        argument_list(sink, payload, indent);
    }
}

/// `name Type`, as written in a struct body.
fn struct_member(sink: &mut Sink, name: &Expression, declared: &Expression, indent: usize) {
    render(sink, name, indent, precedence::PRIMARY);
    sink.emit(" ");
    render(sink, declared, indent, precedence::PRIMARY);
}

/// `name: value`, as written at a call site.
fn named_argument(sink: &mut Sink, name: &str, value: &Expression, indent: usize) {
    sink.emit(name);
    sink.emit(": ");
    expression(sink, value, indent);
}

/// `value as Type`
fn cast(sink: &mut Sink, value: &Expression, target: &Expression, indent: usize) {
    render(sink, value, indent, precedence::POSTFIX);
    sink.emit(" as ");
    render(sink, target, indent, precedence::PRIMARY);
}

/// `name`, or `Qualifier.name` when the reference carries a class qualifier.
fn identifier(sink: &mut Sink, name: &str, qualifier: Option<&str>) {
    if let Some(qualifier) = qualifier {
        sink.emit(qualifier);
        sink.emit(".");
    }
    sink.emit(name);
}

/// `left op right`, wrapping either side that binds more loosely.
///
/// Every binary operator in the grammar is left-associative, so the right
/// operand needs one level more strength than the left to stay unwrapped.
fn binary(
    sink: &mut Sink,
    left: &Expression,
    operator: BinaryOp,
    right: &Expression,
    indent: usize,
) {
    let strength = binary_precedence(operator);
    // A range does not chain, so neither side may re-associate.
    let left_minimum = if operator == BinaryOp::Range {
        strength + 1
    } else {
        strength
    };
    render(sink, left, indent, left_minimum);
    sink.emit(" ");
    sink.emit(binary_operator(operator));
    sink.emit(" ");
    render(sink, right, indent, strength + 1);
}

/// A prefix or postfix unary operator applied to its operand.
fn unary(
    sink: &mut Sink,
    operator: crate::ast::operator::UnaryOp,
    operand: &Expression,
    indent: usize,
) {
    if is_postfix_unary(operator) {
        render(sink, operand, indent, precedence::POSTFIX);
        sink.emit(unary_operator(operator));
        return;
    }
    sink.emit(unary_operator(operator));
    render(sink, operand, indent, precedence::UNARY);
}

/// The left-hand side of an assignment.
fn assignment_target(sink: &mut Sink, target: &LeftHandSideExpression, indent: usize) {
    match target {
        LeftHandSideExpression::Identifier(expr)
        | LeftHandSideExpression::Member(expr)
        | LeftHandSideExpression::Index(expr) => render(sink, expr, indent, precedence::POSTFIX),
    }
}

/// `value if condition else alternative`, the postfix form the parser reads.
fn conditional(
    sink: &mut Sink,
    value: &Expression,
    condition: &Expression,
    alternative: Option<&Expression>,
    form: &IfStatementType,
    indent: usize,
) {
    render(sink, value, indent, precedence::NULL_COALESCE);
    match form {
        IfStatementType::If => sink.emit(" if "),
        IfStatementType::Unless => sink.emit(" unless "),
    }
    render(sink, condition, indent, precedence::CONDITIONAL);
    if let Some(alternative) = alternative {
        sink.emit(" else ");
        render(sink, alternative, indent, precedence::CONDITIONAL);
    }
}

/// `start..end`, `start..=end`, or a bare iterable.
fn range(
    sink: &mut Sink,
    start: &Expression,
    end: Option<&Expression>,
    form: &RangeExpressionType,
    indent: usize,
) {
    match form {
        RangeExpressionType::IterableObject => {
            expression(sink, start, indent);
        }
        RangeExpressionType::Exclusive | RangeExpressionType::Inclusive => {
            render(sink, start, indent, precedence::RANGE + 1);
            sink.emit(match form {
                RangeExpressionType::Inclusive => "..=",
                RangeExpressionType::Exclusive | RangeExpressionType::IterableObject => "..",
            });
            if let Some(end) = end {
                render(sink, end, indent, precedence::RANGE + 1);
            }
        }
    }
}

/// `use a.b`, `use a.*`, `use a.{b, c as d}`.
fn import_path(sink: &mut Sink, segments: &[Expression], form: &ImportPathKind, indent: usize) {
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            sink.emit(".");
        }
        expression(sink, segment, indent);
    }
    match form {
        ImportPathKind::Simple => {}
        ImportPathKind::Wildcard => sink.emit(".*"),
        ImportPathKind::Multi(items) => {
            sink.emit(".{");
            for (index, (item, alias)) in items.iter().enumerate() {
                if index > 0 {
                    sink.emit(", ");
                }
                expression(sink, item, indent);
                if let Some(alias) = alias {
                    sink.emit(" as ");
                    expression(sink, alias, indent);
                }
            }
            sink.emit("}");
        }
    }
}

/// `Name<G> declaration Bound`, as written in a generic parameter list.
fn type_declaration(
    sink: &mut Sink,
    name: &Expression,
    generics: Option<&[Expression]>,
    declaration: &crate::ast::types::TypeDeclarationKind,
    bound: Option<&Expression>,
    indent: usize,
) {
    render(sink, name, indent, precedence::PRIMARY);
    generic_arguments(sink, generics);
    let Some(bound) = bound else {
        return;
    };
    match declaration {
        crate::ast::types::TypeDeclarationKind::None => sink.emit(" "),
        crate::ast::types::TypeDeclarationKind::Is
        | crate::ast::types::TypeDeclarationKind::Extends
        | crate::ast::types::TypeDeclarationKind::Implements
        | crate::ast::types::TypeDeclarationKind::Includes => {
            sink.emit(" ");
            sink.emit(&declaration.to_string());
            sink.emit(" ");
        }
    }
    render(sink, bound, indent, precedence::PRIMARY);
}

/// An anonymous function or arrow lambda.
fn lambda(sink: &mut Sink, data: &LambdaData, indent: usize) {
    sink.emit("fn");
    function_modifiers_suffix(sink, data);
    generic_arguments(sink, data.generics.as_deref());
    parameter_list(sink, &data.params);
    if let Some(return_type) = &data.return_type {
        sink.emit(" ");
        render(sink, return_type, indent, precedence::PRIMARY);
    }
    super::statement::lambda_body(sink, &data.body, indent);
}

/// Modifiers that a lambda carries, such as `async`.
fn function_modifiers_suffix(sink: &mut Sink, data: &LambdaData) {
    let mut modifiers = Sink::new();
    function_modifiers(&mut modifiers, &data.properties);
    if modifiers.is_empty() {
        return;
    }
    sink.emit(" ");
    sink.emit(modifiers.text().trim_end());
}

/// A parenthesised argument list.
fn argument_list(sink: &mut Sink, arguments: &[Expression], indent: usize) {
    sink.emit("(");
    for (index, argument) in arguments.iter().enumerate() {
        if index > 0 {
            sink.emit(", ");
        }
        expression(sink, argument, indent);
    }
    sink.emit(")");
}

/// A bracketed, braced, or parenthesised sequence of elements.
fn sequence(sink: &mut Sink, open: &str, elements: &[Expression], close: &str, indent: usize) {
    sink.emit(open);
    for (index, element) in elements.iter().enumerate() {
        if index > 0 {
            sink.emit(", ");
        }
        expression(sink, element, indent);
    }
    sink.emit(close);
}

/// `{key: value, ...}`, or `{}` when empty.
fn map(sink: &mut Sink, entries: &[(Expression, Expression)], indent: usize) {
    sink.emit("{");
    for (index, (key, value)) in entries.iter().enumerate() {
        if index > 0 {
            sink.emit(", ");
        }
        expression(sink, key, indent);
        sink.emit(": ");
        expression(sink, value, indent);
    }
    sink.emit("}");
}

/// An interpolated string: literal parts are text, the rest are `{...}` holes.
fn formatted_string(sink: &mut Sink, parts: &[Expression], indent: usize) {
    sink.emit("f\"");
    for part in parts {
        // A literal segment is the text between holes; anything else is a hole.
        if let ExpressionKind::Literal(crate::ast::literal::Literal::String(text)) = &part.node {
            sink.emit(&super::helpers::escape_string(text));
            continue;
        }
        sink.emit("{");
        expression(sink, part, indent);
        sink.emit("}");
    }
    sink.emit("\"");
}

/// A block whose last expression is its value.
fn block(sink: &mut Sink, statements: &[crate::ast::Statement], value: &Expression, indent: usize) {
    for entry in statements {
        sink.emit_line(indent + 1);
        format_statement(sink, entry, indent + 1);
    }
    sink.emit_line(indent + 1);
    expression(sink, value, indent + 1);
}
