// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Pure-AST analysis: GPU usage detection and other structural queries.

use crate::ast::common::Parameter;
use crate::ast::expression::{Expression, ExpressionKind, LeftHandSideExpression};
use crate::ast::literal::Literal;
use crate::ast::pattern::{MatchBranch, Pattern};
use crate::ast::statement::{AcceleratorTarget, Statement, StatementKind};

/// Returns true if any statement in the iterable uses GPU constructs.
pub fn program_uses_gpu<'a, I: IntoIterator<Item = &'a Statement>>(stmts: I) -> bool {
    for stmt in stmts {
        if stmt_uses_gpu(stmt) {
            return true;
        }
    }
    false
}

/// Returns true if a statement or any of its nested children use GPU constructs.
fn stmt_uses_gpu(stmt: &Statement) -> bool {
    match &stmt.node {
        StatementKind::Forall { device, .. } => {
            matches!(device, AcceleratorTarget::Gpu)
        }
        StatementKind::GpuFrame(_, _, _) => true,
        StatementKind::GpuFrameBlock(block) => stmt_uses_gpu(block),
        StatementKind::FunctionDeclaration(decl) => {
            decl.properties.is_gpu || decl.body.as_ref().is_some_and(|b| stmt_uses_gpu(b))
        }
        StatementKind::Block(stmts) => stmts.iter().any(stmt_uses_gpu),
        StatementKind::If(_, then_branch, else_branch, _) => {
            stmt_uses_gpu(then_branch) || else_branch.as_ref().is_some_and(|s| stmt_uses_gpu(s))
        }
        StatementKind::While(_, body, _) | StatementKind::For(_, _, body) => stmt_uses_gpu(body),
        StatementKind::Class(class_data) => class_data.body.iter().any(stmt_uses_gpu),
        StatementKind::Struct(_, _, _, methods, _, _)
        | StatementKind::Enum(_, _, _, methods, _, _)
        | StatementKind::Trait(_, _, _, methods, _) => methods.iter().any(stmt_uses_gpu),
        // A `gpu let` / `gpu var` binding may trigger a cross-residency
        // readback that calls into the GPU runtime even when the program has
        // no `forall` / `gpu fn`, so a gpu-resident declaration alone
        // requires linking it.
        StatementKind::Variable(decls, _) => decls
            .iter()
            .any(|d| d.residency == crate::ast::statement::BindingResidency::Gpu),
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Expression(_)
        | StatementKind::Return(_)
        | StatementKind::Use(_, _)
        | StatementKind::Type(_, _)
        | StatementKind::RuntimeFunctionDeclaration(..)
        | StatementKind::IntrinsicFunctionDeclaration(..) => false,
    }
}

/// Whether `statements` write a regex literal (`re"..."`) or a regex match
/// pattern anywhere, nested declarations and lambdas included.
///
/// The type checker loads the module that builds and tests regex values only
/// for a file that answers yes, so a program without regex syntax never has
/// that module's names, or the names it depends on, loaded behind its back.
pub fn program_uses_regex(statements: &[Statement]) -> bool {
    statements.iter().any(statement_uses_regex)
}

fn statement_uses_regex(statement: &Statement) -> bool {
    match &statement.node {
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Use(_, _)
        | StatementKind::Type(_, _)
        | StatementKind::RuntimeFunctionDeclaration(..)
        | StatementKind::IntrinsicFunctionDeclaration(..) => false,
        StatementKind::Expression(expression) => expression_uses_regex(expression),
        StatementKind::Block(statements) => program_uses_regex(statements),
        StatementKind::Variable(declarations, _) => declarations
            .iter()
            .any(|declaration| optional_uses_regex(declaration.initializer.as_deref())),
        StatementKind::If(condition, then_branch, else_branch, _) => {
            expression_uses_regex(condition)
                || statement_uses_regex(then_branch)
                || else_branch.as_deref().is_some_and(statement_uses_regex)
        }
        StatementKind::While(condition, body, _) => {
            expression_uses_regex(condition) || statement_uses_regex(body)
        }
        StatementKind::For(_, iterable, body)
        | StatementKind::GpuFrame(_, iterable, body)
        | StatementKind::Forall { iterable, body, .. } => {
            expression_uses_regex(iterable) || statement_uses_regex(body)
        }
        StatementKind::GpuFrameBlock(body) => statement_uses_regex(body),
        StatementKind::FunctionDeclaration(declaration) => {
            parameters_use_regex(&declaration.params)
                || declaration
                    .body
                    .as_deref()
                    .is_some_and(statement_uses_regex)
        }
        StatementKind::Return(value) => optional_uses_regex(value.as_deref()),
        StatementKind::Enum(_, _, _, methods, _, _)
        | StatementKind::Struct(_, _, _, methods, _, _)
        | StatementKind::Trait(_, _, _, methods, _) => program_uses_regex(methods),
        StatementKind::Class(class) => program_uses_regex(&class.body),
    }
}

fn expression_uses_regex(expression: &Expression) -> bool {
    match &expression.node {
        ExpressionKind::Literal(literal) => matches!(literal, Literal::Regex(_)),
        ExpressionKind::Identifier(_, _)
        | ExpressionKind::Super
        | ExpressionKind::ImportPath(_, _)
        | ExpressionKind::Type(_, _) => false,
        ExpressionKind::Binary(left, _, right)
        | ExpressionKind::Logical(left, _, right)
        | ExpressionKind::Member(left, right)
        | ExpressionKind::Index(left, right)
        | ExpressionKind::StructMember(left, right)
        | ExpressionKind::Cast(left, right) => {
            expression_uses_regex(left) || expression_uses_regex(right)
        }
        ExpressionKind::Unary(_, operand)
        | ExpressionKind::Guard(_, operand)
        | ExpressionKind::NamedArgument(_, operand) => expression_uses_regex(operand),
        ExpressionKind::Assignment(target, _, value) => {
            left_hand_side_uses_regex(target) || expression_uses_regex(value)
        }
        ExpressionKind::Conditional(condition, then_value, else_value, _) => {
            expression_uses_regex(condition)
                || expression_uses_regex(then_value)
                || optional_uses_regex(else_value.as_deref())
        }
        ExpressionKind::Range(start, end, _) => {
            expression_uses_regex(start) || optional_uses_regex(end.as_deref())
        }
        ExpressionKind::Call(target, arguments) | ExpressionKind::EnumValue(target, arguments) => {
            expression_uses_regex(target) || arguments.iter().any(expression_uses_regex)
        }
        ExpressionKind::GenericType(name, bounds, _) => {
            expression_uses_regex(name) || bounds.iter().any(expression_uses_regex)
        }
        ExpressionKind::TypeDeclaration(..) => false,
        ExpressionKind::Lambda(lambda) => {
            parameters_use_regex(&lambda.params) || statement_uses_regex(&lambda.body)
        }
        ExpressionKind::List(items)
        | ExpressionKind::Tuple(items)
        | ExpressionKind::Set(items)
        | ExpressionKind::FormattedString(items, _) => items.iter().any(expression_uses_regex),
        ExpressionKind::Array(items, size) => {
            items.iter().any(expression_uses_regex) || expression_uses_regex(size)
        }
        ExpressionKind::Map(entries) => entries
            .iter()
            .any(|(key, value)| expression_uses_regex(key) || expression_uses_regex(value)),
        ExpressionKind::Match(subject, branches) => {
            expression_uses_regex(subject) || branches.iter().any(branch_uses_regex)
        }
        ExpressionKind::Block(statements, tail) => {
            program_uses_regex(statements) || expression_uses_regex(tail)
        }
    }
}

fn optional_uses_regex(expression: Option<&Expression>) -> bool {
    expression.is_some_and(expression_uses_regex)
}

fn parameters_use_regex(parameters: &[Parameter]) -> bool {
    parameters
        .iter()
        .any(|parameter| optional_uses_regex(parameter.default_value.as_deref()))
}

fn left_hand_side_uses_regex(target: &LeftHandSideExpression) -> bool {
    match target {
        LeftHandSideExpression::Identifier(target)
        | LeftHandSideExpression::Member(target)
        | LeftHandSideExpression::Index(target) => expression_uses_regex(target),
    }
}

fn branch_uses_regex(branch: &MatchBranch) -> bool {
    branch.patterns.iter().any(pattern_uses_regex)
        || optional_uses_regex(branch.guard.as_deref())
        || statement_uses_regex(&branch.body)
}

fn pattern_uses_regex(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Regex(_) => true,
        Pattern::Literal(literal) => matches!(literal, Literal::Regex(_)),
        Pattern::Identifier(_) | Pattern::Default => false,
        Pattern::Tuple(patterns) => patterns.iter().any(pattern_uses_regex),
        Pattern::Member(parent, _) => pattern_uses_regex(parent),
        Pattern::EnumVariant(path, bindings) => {
            pattern_uses_regex(path) || bindings.iter().any(pattern_uses_regex)
        }
    }
}
