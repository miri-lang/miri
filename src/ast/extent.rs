// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How far a statement reaches in the source text.
//!
//! A declaration statement is assembled by the AST factory, which has no source
//! text of its own, so the statement's own span is empty and carries neither
//! end. The real positions live on the expressions and nested statements
//! underneath it. Finding where a declaration begins and stops therefore means
//! visiting everything it contains and keeping the widest range seen.
//!
//! Nothing here bounds its own recursion. It walks trees the parser produced,
//! and the parser refuses to build one deeper than its own limit, so the depth
//! reached here is bounded by that. A caller handing this module a tree from
//! anywhere else would have to bound it.
//!
//! Every match here is exhaustive on purpose. A new `StatementKind` or
//! `ExpressionKind` that this module does not visit would silently shorten the
//! reported extent rather than fail, so the compiler is made to point at this
//! file instead.

use crate::ast::attributes::Attribute;
use crate::ast::common::Parameter;
use crate::ast::expression::{
    Expression, ExpressionKind, ImportPathKind, LambdaData, LeftHandSideExpression,
};
use crate::ast::pattern::MatchBranch;
use crate::ast::statement::{
    ClassData, FunctionDeclarationData, Statement, StatementKind, VariableDeclaration,
};
use crate::error::syntax::Span;

/// The source range `statement` covers, or `None` when nothing under it was
/// parsed from source text.
///
/// A node the factory built carries an empty span, which is skipped rather than
/// counted, so a synthesized node sits harmlessly inside a statement that was
/// parsed from real text.
pub fn source_extent(statement: &Statement) -> Option<Span> {
    let mut reach = Reach::default();
    visit_statement(statement, &mut reach);
    reach.span()
}

/// The widest source range seen while walking a subtree.
#[derive(Default)]
struct Reach {
    start: Option<usize>,
    end: usize,
}

impl Reach {
    /// The range covering everything seen, or `None` when nothing was.
    fn span(&self) -> Option<Span> {
        self.start.map(|start| Span::new(start, self.end))
    }
}

/// Widen the reach to include `span`, ignoring the empty span of a node that
/// was built rather than parsed.
fn reach(furthest: &mut Reach, span: Span) {
    if span.end == 0 {
        return;
    }
    furthest.start = Some(match furthest.start {
        Some(start) => start.min(span.start),
        None => span.start,
    });
    if span.end > furthest.end {
        furthest.end = span.end;
    }
}

/// Visit a statement and everything it declares or contains.
fn visit_statement(statement: &Statement, furthest: &mut Reach) {
    reach(furthest, statement.span);

    match &statement.node {
        StatementKind::Empty | StatementKind::Break | StatementKind::Continue => {}
        StatementKind::Expression(expression) => visit_expression(expression, furthest),
        StatementKind::Block(statements) => visit_statements(statements, furthest),
        StatementKind::Variable(declarations, _) => visit_variables(declarations, furthest),
        StatementKind::If(condition, then_branch, else_branch, _) => {
            visit_expression(condition, furthest);
            visit_statement(then_branch, furthest);
            visit_optional_statement(else_branch.as_deref(), furthest);
        }
        StatementKind::While(condition, body, _) => {
            visit_expression(condition, furthest);
            visit_statement(body, furthest);
        }
        StatementKind::For(vars, iterable, body) => visit_iteration(vars, iterable, body, furthest),
        StatementKind::Forall {
            device: _,
            vars,
            iterable,
            body,
        } => visit_iteration(vars, iterable, body, furthest),
        StatementKind::GpuFrame(vars, iterable, body) => {
            visit_iteration(vars, iterable, body, furthest)
        }
        StatementKind::GpuFrameBlock(body) => visit_statement(body, furthest),
        StatementKind::FunctionDeclaration(declaration) => visit_function(declaration, furthest),
        StatementKind::Return(value) => visit_optional_expression(value.as_deref(), furthest),
        StatementKind::Use(path, alias) => {
            visit_expression(path, furthest);
            visit_optional_expression(alias.as_deref(), furthest);
        }
        StatementKind::Type(declarations, _) => visit_expressions(declarations, furthest),
        StatementKind::Enum(name, generics, variants, methods, _, attributes) => {
            visit_container(name, generics.as_deref(), variants, methods, furthest);
            visit_attributes(attributes, furthest);
        }
        StatementKind::Struct(name, generics, fields, methods, _, traits) => {
            visit_container(name, generics.as_deref(), fields, methods, furthest);
            visit_expressions(traits, furthest);
        }
        StatementKind::Class(data) => visit_class(data, furthest),
        StatementKind::Trait(name, generics, parents, body, _) => {
            visit_container(name, generics.as_deref(), parents, body, furthest)
        }
        StatementKind::RuntimeFunctionDeclaration(_, _, params, return_type) => {
            visit_signature(None, params, return_type.as_deref(), furthest)
        }
        StatementKind::IntrinsicFunctionDeclaration(_, generics, params, return_type, _) => {
            visit_signature(
                generics.as_deref(),
                params,
                return_type.as_deref(),
                furthest,
            )
        }
    }
}

/// Visit a loop's bindings, the thing it walks, and the body it runs.
fn visit_iteration(
    vars: &[VariableDeclaration],
    iterable: &Expression,
    body: &Statement,
    furthest: &mut Reach,
) {
    visit_variables(vars, furthest);
    visit_expression(iterable, furthest);
    visit_statement(body, furthest);
}

/// Visit a function's attributes, signature and body.
fn visit_function(declaration: &FunctionDeclarationData, furthest: &mut Reach) {
    reach(furthest, declaration.name_span);
    visit_attributes(&declaration.attributes, furthest);
    visit_signature(
        declaration.generics.as_deref(),
        &declaration.params,
        declaration.return_type.as_deref(),
        furthest,
    );
    visit_optional_statement(declaration.body.as_deref(), furthest);
}

/// Visit the generics, parameters and return type a function declares.
fn visit_signature(
    generics: Option<&[Expression]>,
    params: &[Parameter],
    return_type: Option<&Expression>,
    furthest: &mut Reach,
) {
    visit_expression_list(generics, furthest);
    visit_parameters(params, furthest);
    visit_optional_expression(return_type, furthest);
}

/// Visit a named container: its name, generics, member expressions and methods.
fn visit_container(
    name: &Expression,
    generics: Option<&[Expression]>,
    members: &[Expression],
    methods: &[Statement],
    furthest: &mut Reach,
) {
    visit_expression(name, furthest);
    visit_expression_list(generics, furthest);
    visit_expressions(members, furthest);
    visit_statements(methods, furthest);
}

/// Visit a class, which additionally names a base class and carries attributes.
fn visit_class(data: &ClassData, furthest: &mut Reach) {
    visit_container(
        &data.name,
        data.generics.as_deref(),
        &data.traits,
        &data.body,
        furthest,
    );
    visit_optional_expression(data.base_class.as_deref(), furthest);
    visit_attributes(&data.attributes, furthest);
}

/// Visit an expression and every expression or statement nested within it.
///
/// A type written as an expression is not descended into: the wrapping
/// expression's own span already covers the whole type, which has no statements
/// inside it to reach further.
fn visit_expression(expression: &Expression, furthest: &mut Reach) {
    reach(furthest, expression.span);

    match &expression.node {
        ExpressionKind::Literal(_)
        | ExpressionKind::Identifier(..)
        | ExpressionKind::Super
        | ExpressionKind::Type(..) => {}
        ExpressionKind::Binary(left, _, right)
        | ExpressionKind::Logical(left, _, right)
        | ExpressionKind::Member(left, right)
        | ExpressionKind::Index(left, right)
        | ExpressionKind::StructMember(left, right)
        | ExpressionKind::Cast(left, right) => {
            visit_expression(left, furthest);
            visit_expression(right, furthest);
        }
        ExpressionKind::Unary(_, operand)
        | ExpressionKind::Guard(_, operand)
        | ExpressionKind::NamedArgument(_, operand) => visit_expression(operand, furthest),
        ExpressionKind::Assignment(target, _, value) => {
            visit_left_hand_side(target, furthest);
            visit_expression(value, furthest);
        }
        ExpressionKind::Conditional(condition, then_value, else_value, _) => {
            visit_expression(condition, furthest);
            visit_expression(then_value, furthest);
            visit_optional_expression(else_value.as_deref(), furthest);
        }
        ExpressionKind::Range(start, end, _) => {
            visit_expression(start, furthest);
            visit_optional_expression(end.as_deref(), furthest);
        }
        ExpressionKind::Call(callee, arguments) | ExpressionKind::EnumValue(callee, arguments) => {
            visit_expression(callee, furthest);
            visit_expressions(arguments, furthest);
        }
        ExpressionKind::ImportPath(segments, kind) => visit_import_path(segments, kind, furthest),
        ExpressionKind::GenericType(base, argument, _) => {
            visit_expression(base, furthest);
            visit_optional_expression(argument.as_deref(), furthest);
        }
        ExpressionKind::TypeDeclaration(name, generics, _, bound) => {
            visit_expression(name, furthest);
            visit_expression_list(generics.as_deref(), furthest);
            visit_optional_expression(bound.as_deref(), furthest);
        }
        ExpressionKind::Lambda(data) => visit_lambda(data, furthest),
        ExpressionKind::List(items)
        | ExpressionKind::Tuple(items)
        | ExpressionKind::Set(items)
        | ExpressionKind::FormattedString(items) => visit_expressions(items, furthest),
        ExpressionKind::Array(items, size) => {
            visit_expressions(items, furthest);
            visit_expression(size, furthest);
        }
        ExpressionKind::Map(entries) => visit_map(entries, furthest),
        ExpressionKind::Match(subject, branches) => {
            visit_expression(subject, furthest);
            visit_branches(branches, furthest);
        }
        ExpressionKind::Block(statements, result) => {
            visit_statements(statements, furthest);
            visit_expression(result, furthest);
        }
    }
}

/// Visit the segments of an import path and whatever its braces list.
fn visit_import_path(segments: &[Expression], kind: &ImportPathKind, furthest: &mut Reach) {
    visit_expressions(segments, furthest);
    match kind {
        ImportPathKind::Simple | ImportPathKind::Wildcard => {}
        ImportPathKind::Multi(entries) => {
            for (item, alias) in entries {
                visit_expression(item, furthest);
                visit_optional_expression(alias.as_deref(), furthest);
            }
        }
    }
}

/// Visit a lambda's signature and the body it closes over.
fn visit_lambda(data: &LambdaData, furthest: &mut Reach) {
    visit_signature(
        data.generics.as_deref(),
        &data.params,
        data.return_type.as_deref(),
        furthest,
    );
    visit_statement(&data.body, furthest);
}

/// Visit both sides of every entry in a map literal.
fn visit_map(entries: &[(Expression, Expression)], furthest: &mut Reach) {
    for (key, value) in entries {
        visit_expression(key, furthest);
        visit_expression(value, furthest);
    }
}

/// Visit the expression a match branch guards on and the body it runs.
///
/// Patterns carry no span, so a branch reaches exactly as far as its guard and
/// its body do.
fn visit_branches(branches: &[MatchBranch], furthest: &mut Reach) {
    for branch in branches {
        visit_optional_expression(branch.guard.as_deref(), furthest);
        visit_statement(&branch.body, furthest);
    }
}

/// Visit the expression behind an assignment target.
fn visit_left_hand_side(target: &LeftHandSideExpression, furthest: &mut Reach) {
    match target {
        LeftHandSideExpression::Identifier(expression)
        | LeftHandSideExpression::Member(expression)
        | LeftHandSideExpression::Index(expression) => visit_expression(expression, furthest),
    }
}

/// Visit every statement in a body.
fn visit_statements(statements: &[Statement], furthest: &mut Reach) {
    for statement in statements {
        visit_statement(statement, furthest);
    }
}

/// Visit a statement that may be absent.
fn visit_optional_statement(statement: Option<&Statement>, furthest: &mut Reach) {
    if let Some(statement) = statement {
        visit_statement(statement, furthest);
    }
}

/// Visit every expression in a list.
fn visit_expressions(expressions: &[Expression], furthest: &mut Reach) {
    for expression in expressions {
        visit_expression(expression, furthest);
    }
}

/// Visit an expression that may be absent.
fn visit_optional_expression(expression: Option<&Expression>, furthest: &mut Reach) {
    if let Some(expression) = expression {
        visit_expression(expression, furthest);
    }
}

/// Visit a generic parameter list that may be absent.
fn visit_expression_list(expressions: Option<&[Expression]>, furthest: &mut Reach) {
    if let Some(expressions) = expressions {
        visit_expressions(expressions, furthest);
    }
}

/// Visit each parameter's type, guard and default value.
fn visit_parameters(parameters: &[Parameter], furthest: &mut Reach) {
    for parameter in parameters {
        visit_expression(&parameter.typ, furthest);
        visit_optional_expression(parameter.guard.as_deref(), furthest);
        visit_optional_expression(parameter.default_value.as_deref(), furthest);
    }
}

/// Visit each declared variable's type and initializer.
fn visit_variables(declarations: &[VariableDeclaration], furthest: &mut Reach) {
    for declaration in declarations {
        visit_optional_expression(declaration.typ.as_deref(), furthest);
        visit_optional_expression(declaration.initializer.as_deref(), furthest);
    }
}

/// Visit each attribute's own span.
fn visit_attributes(attributes: &[Attribute], furthest: &mut Reach) {
    for attribute in attributes {
        reach(furthest, attribute.span);
    }
}
