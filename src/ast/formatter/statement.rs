// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Renders statements and declarations back to Miri source syntax.
//!
//! A statement renders at the cursor its caller left, and uses `indent` only
//! for the lines it opens itself. Bodies always take the indented block form
//! rather than the single-line `:` form, so nesting depth is carried by one
//! rule instead of two.

use crate::ast::attributes::{Attribute, AttributeSpelling};
use crate::ast::common::{MemberVisibility, Parameter, RuntimeKind};
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::pattern::MatchBranch;
use crate::ast::statement::{
    AcceleratorTarget, BindingResidency, ClassData, FunctionDeclarationData, IfStatementType,
    Statement, StatementKind, VariableDeclaration, VariableDeclarationType, WhileStatementType,
};

use super::expression::expression as format_expression;
use super::helpers::{function_modifiers, parameter_list, visibility};
use super::pattern::pattern as format_pattern;
use super::sink::{Mark, Sink};
use super::types::generic_arguments;

/// Render a statement at the caller's cursor.
pub fn statement(sink: &mut Sink, node: &Statement, indent: usize) {
    match &node.node {
        StatementKind::Empty => {}
        StatementKind::Break => sink.emit("break"),
        StatementKind::Continue => sink.emit("continue"),
        StatementKind::Expression(value) => format_expression(sink, value, indent),
        StatementKind::Block(statements) => statement_lines(sink, statements, indent),
        StatementKind::Variable(declarations, level) => variable(sink, declarations, level, indent),
        StatementKind::If(condition, then, otherwise, form) => {
            conditional(sink, condition, then, otherwise.as_deref(), form, indent)
        }
        StatementKind::While(condition, loop_body, form) => {
            while_loop(sink, condition, loop_body, form, indent)
        }
        StatementKind::For(..) | StatementKind::Forall { .. } | StatementKind::GpuFrame(..) => {
            iteration_statement(sink, node, indent)
        }
        StatementKind::GpuFrameBlock(passes) => {
            sink.emit("gpu frame");
            body(sink, passes, indent);
        }
        StatementKind::FunctionDeclaration(declaration) => function(sink, declaration, indent),
        StatementKind::Return(value) => return_statement(sink, value.as_deref(), indent),
        StatementKind::Use(path, alias) => use_statement(sink, path, alias.as_deref(), indent),
        StatementKind::Type(declarations, level) => type_alias(sink, declarations, level, indent),
        StatementKind::Enum(name, generics, variants, methods, level, attributes) => {
            enum_declaration(
                sink, name, generics, variants, methods, level, attributes, indent,
            )
        }
        StatementKind::Struct(name, generics, fields, methods, level, traits) => {
            struct_declaration(sink, name, generics, fields, methods, level, traits, indent)
        }
        StatementKind::Class(data) => class(sink, data, indent),
        StatementKind::Trait(name, generics, parents, members, level) => {
            trait_declaration(sink, name, generics, parents, members, level, indent)
        }
        StatementKind::RuntimeFunctionDeclaration(runtime, name, parameters, return_type) => {
            runtime_function(
                sink,
                runtime,
                name,
                parameters,
                return_type.as_deref(),
                indent,
            )
        }
        StatementKind::IntrinsicFunctionDeclaration(
            name,
            generics,
            parameters,
            return_type,
            level,
        ) => intrinsic_function(
            sink,
            name,
            generics.as_deref(),
            parameters,
            return_type.as_deref(),
            level,
            indent,
        ),
    }
}

/// Render a loop that walks an iteration clause.
fn iteration_statement(sink: &mut Sink, node: &Statement, indent: usize) {
    if let StatementKind::For(variables, iterable, loop_body) = &node.node {
        keyed_iteration(sink, "for ", variables, iterable, loop_body, indent);
        return;
    }
    if let StatementKind::Forall {
        device,
        vars,
        iterable,
        body: loop_body,
    } = &node.node
    {
        keyed_iteration(
            sink,
            forall_keyword(device),
            vars,
            iterable,
            loop_body,
            indent,
        );
        return;
    }
    if let StatementKind::GpuFrame(variables, iterable, loop_body) = &node.node {
        keyed_iteration(sink, "gpu frame ", variables, iterable, loop_body, indent);
    }
}

/// The keyword a `forall` opens with, which names its target.
fn forall_keyword(device: &AcceleratorTarget) -> &'static str {
    match device {
        AcceleratorTarget::Gpu => "gpu forall ",
        AcceleratorTarget::Inferred => "forall ",
    }
}

/// A loop header introduced by `keyword`, followed by its body.
fn keyed_iteration(
    sink: &mut Sink,
    keyword: &str,
    variables: &[VariableDeclaration],
    iterable: &Expression,
    loop_body: &Statement,
    indent: usize,
) {
    sink.emit(keyword);
    iteration(sink, variables, iterable, loop_body, indent);
}

/// `return`, with the value it carries.
fn return_statement(sink: &mut Sink, value: Option<&Expression>, indent: usize) {
    sink.emit("return");
    if let Some(value) = value {
        sink.emit(" ");
        format_expression(sink, value, indent);
    }
}

/// `use path`, with the alias it was imported under.
fn use_statement(sink: &mut Sink, path: &Expression, alias: Option<&Expression>, indent: usize) {
    sink.emit("use ");
    format_expression(sink, path, indent);
    if let Some(alias) = alias {
        sink.emit(" as ");
        format_expression(sink, alias, indent);
    }
}

/// `type Name is Type`
fn type_alias(
    sink: &mut Sink,
    declarations: &[Expression],
    level: &MemberVisibility,
    indent: usize,
) {
    visibility(sink, level);
    sink.emit("type ");
    comma_separated(sink, declarations, indent);
}

/// Render a body below the header the caller emitted.
///
/// The grammar offers two forms — `body <- COLON statement / block` — and the
/// parser records which was written: the block form yields a `Block`, the colon
/// form yields the bare statement. Rendering mirrors that, so a body written
/// inline comes back inline and the tree is unchanged. Rendering everything as
/// a block would wrap a colon body in a `Block` it never had.
pub fn body(sink: &mut Sink, node: &Statement, indent: usize) {
    let StatementKind::Block(statements) = &node.node else {
        sink.emit(": ");
        statement(sink, node, indent);
        return;
    };
    for entry in statements {
        sink.emit_line(indent + 1);
        statement(sink, entry, indent + 1);
    }
}

/// Render an anonymous function's body.
///
/// A newline inside brackets produces no indentation token, so a block body
/// cannot re-parse where a lambda sits inside an argument list. A body that is
/// a single expression therefore takes the inline `: value` form, which parses
/// in either position.
pub fn lambda_body(sink: &mut Sink, node: &Statement, indent: usize) {
    if matches!(node.node, StatementKind::Empty) {
        return;
    }
    if let Some(value) = single_expression(node) {
        sink.emit(": ");
        format_expression(sink, value, indent);
        return;
    }
    body(sink, node, indent);
}

/// The single expression a body consists of, when it consists of exactly one.
fn single_expression(node: &Statement) -> Option<&Expression> {
    if let StatementKind::Expression(value) = &node.node {
        return Some(value);
    }
    if let StatementKind::Block(statements) = &node.node {
        if let [only] = statements.as_slice() {
            return single_expression(only);
        }
    }
    None
}

/// Render statements one per line at `indent`, the first at the cursor.
fn statement_lines(sink: &mut Sink, statements: &[Statement], indent: usize) {
    for (index, entry) in statements.iter().enumerate() {
        if index > 0 {
            sink.emit_line(indent);
        }
        statement(sink, entry, indent);
    }
}

/// Render expressions separated by `, `.
fn comma_separated(sink: &mut Sink, expressions: &[Expression], indent: usize) {
    for (index, entry) in expressions.iter().enumerate() {
        if index > 0 {
            sink.emit(", ");
        }
        format_expression(sink, entry, indent);
    }
}

/// `let a T = x, b = y`
fn variable(
    sink: &mut Sink,
    declarations: &[VariableDeclaration],
    level: &MemberVisibility,
    indent: usize,
) {
    visibility(sink, level);
    let Some(first) = declarations.first() else {
        return;
    };
    // Residency precedes the binding keyword: the source form is `gpu let x`.
    if first.residency == BindingResidency::Gpu {
        sink.emit("gpu ");
    }
    if first.is_shared {
        sink.emit("shared ");
    } else {
        sink.emit(match first.declaration_type {
            VariableDeclarationType::Mutable => "var ",
            VariableDeclarationType::Immutable => "let ",
            VariableDeclarationType::Constant => "const ",
        });
    }
    for (index, declaration) in declarations.iter().enumerate() {
        if index > 0 {
            sink.emit(", ");
        }
        binding(sink, declaration, indent);
    }
}

/// One binding in a declaration list: `name Type = value`.
fn binding(sink: &mut Sink, declaration: &VariableDeclaration, indent: usize) {
    sink.emit(&declaration.name);
    if let Some(declared) = &declaration.typ {
        sink.emit(" ");
        format_expression(sink, declared, indent);
    }
    if let Some(initializer) = &declaration.initializer {
        sink.emit(" = ");
        format_expression(sink, initializer, indent);
    }
}

/// `if condition` / `unless condition`, with an optional `else`.
fn conditional(
    sink: &mut Sink,
    condition: &Expression,
    then_branch: &Statement,
    else_branch: Option<&Statement>,
    form: &IfStatementType,
    indent: usize,
) {
    match form {
        IfStatementType::If => sink.emit("if "),
        IfStatementType::Unless => sink.emit("unless "),
    }
    format_expression(sink, condition, indent);
    body(sink, then_branch, indent);
    let Some(else_branch) = else_branch else {
        return;
    };
    sink.emit_line(indent);
    sink.emit("else");
    // `else if` chains stay on the `else` line rather than nesting a block.
    if matches!(else_branch.node, StatementKind::If(..)) {
        sink.emit(" ");
        statement(sink, else_branch, indent);
        return;
    }
    body(sink, else_branch, indent);
}

/// `while` / `until` / `forever` / `do ... while`.
fn while_loop(
    sink: &mut Sink,
    condition: &Expression,
    loop_body: &Statement,
    form: &WhileStatementType,
    indent: usize,
) {
    match form {
        WhileStatementType::While => {
            sink.emit("while ");
            format_expression(sink, condition, indent);
            body(sink, loop_body, indent);
        }
        WhileStatementType::Until => {
            sink.emit("until ");
            format_expression(sink, condition, indent);
            body(sink, loop_body, indent);
        }
        WhileStatementType::Forever => {
            sink.emit("forever");
            body(sink, loop_body, indent);
        }
        WhileStatementType::DoWhile | WhileStatementType::DoUntil => {
            sink.emit("do");
            body(sink, loop_body, indent);
            sink.emit_line(indent);
            sink.emit(match form {
                WhileStatementType::DoUntil => "until ",
                WhileStatementType::DoWhile
                | WhileStatementType::While
                | WhileStatementType::Until
                | WhileStatementType::Forever => "while ",
            });
            format_expression(sink, condition, indent);
        }
    }
}

/// `vars in iterable` followed by the loop body.
fn iteration(
    sink: &mut Sink,
    variables: &[VariableDeclaration],
    iterable: &Expression,
    loop_body: &Statement,
    indent: usize,
) {
    for (index, variable) in variables.iter().enumerate() {
        if index > 0 {
            sink.emit(", ");
        }
        sink.emit(&variable.name);
        if let Some(declared) = &variable.typ {
            sink.emit(" ");
            format_expression(sink, declared, indent);
        }
    }
    sink.emit(" in ");
    iteration_ranges(sink, variables.len(), iterable, indent);
    body(sink, loop_body, indent);
}

/// Render the range side of an iteration clause.
///
/// A multi-dimensional loop carries one range per variable, and the parser
/// collects them into a tuple. The source form lists them bare, so the tuple is
/// unwrapped rather than rendered with its parentheses.
fn iteration_ranges(sink: &mut Sink, dimensions: usize, iterable: &Expression, indent: usize) {
    if dimensions > 1 {
        if let ExpressionKind::Tuple(ranges) = &iterable.node {
            if ranges.len() == dimensions {
                comma_separated(sink, ranges, indent);
                return;
            }
        }
    }
    format_expression(sink, iterable, indent);
}

/// `match subject` and its branches, as an expression in value position.
pub fn match_expression(
    sink: &mut Sink,
    subject: &Expression,
    branches: &[MatchBranch],
    indent: usize,
) {
    sink.emit("match ");
    format_expression(sink, subject, indent);
    for branch in branches {
        sink.emit_line(indent + 1);
        match_branch(sink, branch, indent + 1);
    }
}

/// One match branch: its patterns, an optional guard, then its body.
fn match_branch(sink: &mut Sink, branch: &MatchBranch, indent: usize) {
    if branch.is_mutable {
        sink.emit("var ");
    }
    for (index, entry) in branch.patterns.iter().enumerate() {
        if index > 0 {
            sink.emit(", ");
        }
        format_pattern(sink, entry);
    }
    if let Some(guard) = &branch.guard {
        sink.emit(" if ");
        format_expression(sink, guard, indent);
    }
    body(sink, &branch.body, indent);
}

/// Render the attributes that precede a declaration, one per line.
fn attributes(sink: &mut Sink, entries: &[Attribute], indent: usize) {
    for entry in entries {
        if entry.spelling == AttributeSpelling::DeprecatedKeyword {
            continue;
        }
        sink.emit("@");
        sink.emit(&entry.name);
        if let Some(argument) = &entry.argument {
            sink.emit("(\"");
            sink.emit(&super::helpers::escape_string(argument));
            sink.emit("\")");
        }
        sink.emit_line(indent);
    }
}

/// A function declaration, with its span recorded.
fn function(sink: &mut Sink, declaration: &FunctionDeclarationData, indent: usize) {
    let mark = sink.mark();
    function_header(sink, declaration, indent);
    if let Some(declared_body) = &declaration.body {
        body(sink, declared_body, indent);
    }
    sink.record(mark, "function", Some(declaration.name.clone()));
}

/// Everything a function declares apart from its body.
pub(super) fn function_header(
    sink: &mut Sink,
    declaration: &FunctionDeclarationData,
    indent: usize,
) {
    attributes(sink, &declaration.attributes, indent);
    function_modifiers(sink, &declaration.properties);
    sink.emit("fn ");
    sink.emit(&declaration.name);
    generic_arguments(sink, declaration.generics.as_deref());
    parameter_list(sink, &declaration.params);
    if let Some(return_type) = &declaration.return_type {
        sink.emit(" ");
        format_expression(sink, return_type, indent);
    }
}

/// `runtime "core" fn name(params) Return`
fn runtime_function(
    sink: &mut Sink,
    runtime: &RuntimeKind,
    name: &str,
    parameters: &[Parameter],
    return_type: Option<&Expression>,
    indent: usize,
) {
    let mark = sink.mark();
    sink.emit("runtime \"");
    sink.emit(runtime.name());
    sink.emit("\" fn ");
    sink.emit(name);
    parameter_list(sink, parameters);
    if let Some(return_type) = return_type {
        sink.emit(" ");
        format_expression(sink, return_type, indent);
    }
    sink.record(mark, "function", Some(name.to_string()));
}

/// `intrinsic fn name<G>(params) Return`
fn intrinsic_function(
    sink: &mut Sink,
    name: &str,
    generics: Option<&[Expression]>,
    parameters: &[Parameter],
    return_type: Option<&Expression>,
    level: &MemberVisibility,
    indent: usize,
) {
    let mark = sink.mark();
    visibility(sink, level);
    sink.emit("intrinsic fn ");
    sink.emit(name);
    generic_arguments(sink, generics);
    parameter_list(sink, parameters);
    if let Some(return_type) = return_type {
        sink.emit(" ");
        format_expression(sink, return_type, indent);
    }
    sink.record(mark, "function", Some(name.to_string()));
}

/// A class declaration, with its span recorded.
fn class(sink: &mut Sink, data: &ClassData, indent: usize) {
    let mark = sink.mark();
    class_header(sink, data, indent);
    members(sink, &data.body, indent);
    record_named(sink, mark, "class", &data.name);
}

/// Everything a class declares apart from its members.
pub(super) fn class_header(sink: &mut Sink, data: &ClassData, indent: usize) {
    attributes(sink, &data.attributes, indent);
    visibility(sink, &data.visibility);
    if data.is_abstract {
        sink.emit("abstract ");
    }
    sink.emit("class ");
    format_expression(sink, &data.name, indent);
    generic_arguments(sink, data.generics.as_deref());
    if let Some(base) = &data.base_class {
        sink.emit(" extends ");
        format_expression(sink, base, indent);
    }
    implements(sink, &data.traits, indent);
}

/// An enum declaration: variants first, then any methods.
#[allow(clippy::too_many_arguments)]
fn enum_declaration(
    sink: &mut Sink,
    name: &Expression,
    generics: &Option<Vec<Expression>>,
    variants: &[Expression],
    methods: &[Statement],
    level: &MemberVisibility,
    entries: &[Attribute],
    indent: usize,
) {
    let mark = sink.mark();
    enum_header(sink, name, generics.as_deref(), level, entries, indent);
    for variant in variants {
        sink.emit_line(indent + 1);
        format_expression(sink, variant, indent + 1);
    }
    for method in methods {
        sink.emit_line(indent + 1);
        statement(sink, method, indent + 1);
    }
    record_named(sink, mark, "enum", name);
}

/// A struct declaration: fields first, then any methods.
#[allow(clippy::too_many_arguments)]
fn struct_declaration(
    sink: &mut Sink,
    name: &Expression,
    generics: &Option<Vec<Expression>>,
    fields: &[Expression],
    methods: &[Statement],
    level: &MemberVisibility,
    traits: &[Expression],
    indent: usize,
) {
    let mark = sink.mark();
    struct_header(sink, name, generics.as_deref(), level, traits, indent);
    for field in fields {
        sink.emit_line(indent + 1);
        format_expression(sink, field, indent + 1);
    }
    for method in methods {
        sink.emit_line(indent + 1);
        statement(sink, method, indent + 1);
    }
    record_named(sink, mark, "struct", name);
}

/// A trait declaration and its member signatures.
fn trait_declaration(
    sink: &mut Sink,
    name: &Expression,
    generics: &Option<Vec<Expression>>,
    parents: &[Expression],
    body_members: &[Statement],
    level: &MemberVisibility,
    indent: usize,
) {
    let mark = sink.mark();
    trait_header(sink, name, generics.as_deref(), parents, level, indent);
    members(sink, body_members, indent);
    record_named(sink, mark, "trait", name);
}

/// ` implements A, B`, or nothing when the list is empty.
fn implements(sink: &mut Sink, traits: &[Expression], indent: usize) {
    if traits.is_empty() {
        return;
    }
    sink.emit(" implements ");
    comma_separated(sink, traits, indent);
}

/// The indented member block of a class or trait.
fn members(sink: &mut Sink, body_members: &[Statement], indent: usize) {
    for member in body_members {
        sink.emit_line(indent + 1);
        statement(sink, member, indent + 1);
    }
}

/// Record a span for a declaration whose name is an expression.
fn record_named(sink: &mut Sink, mark: Mark, kind: &str, name: &Expression) {
    let mut rendered = Sink::new();
    format_expression(&mut rendered, name, 0);
    sink.record(mark, kind, Some(rendered.text().to_string()));
}

/// Everything an enum declares apart from its variants and methods.
pub(super) fn enum_header(
    sink: &mut Sink,
    name: &Expression,
    generics: Option<&[Expression]>,
    level: &MemberVisibility,
    entries: &[Attribute],
    indent: usize,
) {
    attributes(sink, entries, indent);
    visibility(sink, level);
    sink.emit("enum ");
    format_expression(sink, name, indent);
    generic_arguments(sink, generics);
}

/// Everything a struct declares apart from its fields and methods.
pub(super) fn struct_header(
    sink: &mut Sink,
    name: &Expression,
    generics: Option<&[Expression]>,
    level: &MemberVisibility,
    traits: &[Expression],
    indent: usize,
) {
    visibility(sink, level);
    sink.emit("struct ");
    format_expression(sink, name, indent);
    generic_arguments(sink, generics);
    implements(sink, traits, indent);
}

/// Everything a trait declares apart from its members.
pub(super) fn trait_header(
    sink: &mut Sink,
    name: &Expression,
    generics: Option<&[Expression]>,
    parents: &[Expression],
    level: &MemberVisibility,
    indent: usize,
) {
    visibility(sink, level);
    sink.emit("trait ");
    format_expression(sink, name, indent);
    generic_arguments(sink, generics);
    if !parents.is_empty() {
        sink.emit(" extends ");
        comma_separated(sink, parents, indent);
    }
}
