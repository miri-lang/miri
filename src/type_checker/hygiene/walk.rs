// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Where a file's scopes and declarations are.
//!
//! Each hygiene check asks about one kind of place: a run of statements that
//! shares a scope, or a declaration that could be residue. Finding those places
//! is one traversal, done once, rather than one per check — the alternative is
//! four walkers that must all be taught about a new statement kind before the
//! checks agree on what a file contains.

use crate::ast::common::{MemberVisibility, Parameter};
use crate::ast::expression::{Expression, ExpressionKind, LambdaData, LeftHandSideExpression};
use crate::ast::pattern::MatchBranch;
use crate::ast::statement::{ClassData, FunctionDeclarationData, Statement, StatementKind};
use std::collections::HashSet;
use std::slice;

/// A declaration that a file could have left behind, with what the report needs
/// to name it.
pub(crate) struct Declaration<'a> {
    /// The declared name.
    pub name: &'a str,
    /// What the declaration is, as the report's noun ("function", "class", …).
    pub noun: &'static str,
    /// Who can reach the name.
    pub visibility: &'a MemberVisibility,
    /// Where the report points.
    pub span: crate::error::syntax::Span,
}

/// A run of statements that shares one scope.
pub(crate) struct Scope<'a> {
    pub statements: &'a [Statement],
    /// True for the file's own top level, where a binding is a declaration the
    /// rest of the module — and anything importing it — can read, rather than a
    /// local.
    pub is_module: bool,
}

/// Everything in a file the hygiene checks look at.
#[derive(Default)]
pub(crate) struct Contents<'a> {
    /// Every run of statements that shares one scope, the file's own top level
    /// included.
    pub scopes: Vec<Scope<'a>>,
    /// Every function that has a body, so its parameters have somewhere to be
    /// read.
    pub functions: Vec<&'a FunctionDeclarationData>,
    /// Every declaration that names something, in the order written.
    pub declarations: Vec<Declaration<'a>>,
}

/// Reads `statements` and everything nested in them.
pub(crate) fn contents(statements: &[Statement]) -> Contents<'_> {
    let mut contents = Contents::default();
    contents.scope(statements, true);
    contents
}

impl<'a> Contents<'a> {
    fn scope(&mut self, statements: &'a [Statement], is_module: bool) {
        self.scopes.push(Scope {
            statements,
            is_module,
        });
        for statement in statements {
            self.statement(statement, is_module);
        }
    }

    /// A body written on one line is a single statement rather than a block,
    /// and it still opens a scope.
    fn body(&mut self, body: &'a Statement) {
        if let StatementKind::Block(statements) = &body.node {
            self.scope(statements, false);
        } else {
            self.scope(slice::from_ref(body), false);
        }
    }

    fn statement(&mut self, statement: &'a Statement, is_module: bool) {
        match &statement.node {
            StatementKind::Empty
            | StatementKind::Break
            | StatementKind::Continue
            | StatementKind::Use(_, _)
            | StatementKind::RuntimeFunctionDeclaration(..)
            | StatementKind::IntrinsicFunctionDeclaration(..) => {}
            StatementKind::Expression(expression) => self.expression(expression),
            StatementKind::Block(statements) => self.scope(statements, is_module),
            StatementKind::Variable(declarations, visibility) => {
                for declaration in declarations {
                    if is_module {
                        self.declare(
                            Some(declaration.name.as_str()),
                            "binding",
                            visibility,
                            statement,
                        );
                    }
                    if let Some(initializer) = &declaration.initializer {
                        self.expression(initializer);
                    }
                }
            }
            StatementKind::If(condition, then_branch, else_branch, _) => {
                self.expression(condition);
                self.body(then_branch);
                if let Some(else_branch) = else_branch {
                    self.body(else_branch);
                }
            }
            StatementKind::While(condition, body, _) => {
                self.expression(condition);
                self.body(body);
            }
            StatementKind::For(_, iterable, body) | StatementKind::GpuFrame(_, iterable, body) => {
                self.expression(iterable);
                self.body(body);
            }
            StatementKind::Forall {
                iterable,
                body,
                vars: _,
                device: _,
            } => {
                self.expression(iterable);
                self.body(body);
            }
            StatementKind::GpuFrameBlock(body) => self.body(body),
            StatementKind::FunctionDeclaration(declaration) => {
                self.function(declaration, statement);
            }
            StatementKind::Return(value) => {
                if let Some(value) = value {
                    self.expression(value);
                }
            }
            StatementKind::Type(parts, visibility) => {
                self.declare(declared_name(parts.first()), "type", visibility, statement);
            }
            StatementKind::Enum(name, _, _, methods, visibility, _) => {
                self.declare(declared_name(Some(name)), "enum", visibility, statement);
                self.members(methods);
            }
            StatementKind::Struct(name, _, _, methods, visibility, _) => {
                self.declare(declared_name(Some(name)), "struct", visibility, statement);
                self.members(methods);
            }
            StatementKind::Class(class) => self.class(class, statement),
            StatementKind::Trait(name, _, _, body, visibility) => {
                self.declare(declared_name(Some(name)), "trait", visibility, statement);
                self.members(body);
            }
        }
    }

    fn class(&mut self, class: &'a ClassData, statement: &'a Statement) {
        self.declare(
            declared_name(Some(&class.name)),
            "class",
            &class.visibility,
            statement,
        );
        self.members(&class.body);
    }

    /// A declaration body is not a scope of its own: its members are
    /// declarations, and each member with a body opens its own.
    fn members(&mut self, members: &'a [Statement]) {
        for member in members {
            self.statement(member, false);
        }
    }

    fn function(&mut self, declaration: &'a FunctionDeclarationData, statement: &'a Statement) {
        self.declarations.push(Declaration {
            name: &declaration.name,
            noun: "function",
            visibility: &declaration.properties.visibility,
            span: name_span(declaration, statement),
        });
        self.parameters(&declaration.params);
        if let Some(body) = &declaration.body {
            self.functions.push(declaration);
            self.body(body);
        }
    }

    fn declare(
        &mut self,
        name: Option<&'a str>,
        noun: &'static str,
        visibility: &'a MemberVisibility,
        statement: &'a Statement,
    ) {
        if let Some(name) = name {
            self.declarations.push(Declaration {
                name,
                noun,
                visibility,
                span: statement.span,
            });
        }
    }

    fn parameters(&mut self, parameters: &'a [Parameter]) {
        for parameter in parameters {
            if let Some(default_value) = &parameter.default_value {
                self.expression(default_value);
            }
        }
    }

    fn lambda(&mut self, lambda: &'a LambdaData) {
        self.parameters(&lambda.params);
        self.body(&lambda.body);
    }

    fn expressions(&mut self, expressions: &'a [Expression]) {
        for expression in expressions {
            self.expression(expression);
        }
    }

    fn expression(&mut self, expression: &'a Expression) {
        match &expression.node {
            ExpressionKind::Literal(_)
            | ExpressionKind::Identifier(_, _)
            | ExpressionKind::Super
            | ExpressionKind::ImportPath(_, _)
            | ExpressionKind::Type(_, _) => {}
            ExpressionKind::Binary(left, _, right)
            | ExpressionKind::Logical(left, _, right)
            | ExpressionKind::Member(left, right)
            | ExpressionKind::Index(left, right)
            | ExpressionKind::StructMember(left, right)
            | ExpressionKind::Cast(left, right) => self.pair(left, right),
            ExpressionKind::Unary(_, operand)
            | ExpressionKind::Guard(_, operand)
            | ExpressionKind::NamedArgument(_, operand) => self.expression(operand),
            ExpressionKind::Assignment(target, _, value) => {
                self.left_hand_side(target);
                self.expression(value);
            }
            ExpressionKind::Conditional(condition, then_value, else_value, _) => {
                self.expression(condition);
                self.expression(then_value);
                self.optional(else_value.as_deref());
            }
            ExpressionKind::Range(start, end, _) => {
                self.expression(start);
                self.optional(end.as_deref());
            }
            ExpressionKind::Call(target, arguments)
            | ExpressionKind::EnumValue(target, arguments) => {
                self.expression(target);
                self.expressions(arguments);
            }
            ExpressionKind::GenericType(name, arguments, _) => {
                self.expression(name);
                self.optional(arguments.as_deref());
            }
            ExpressionKind::TypeDeclaration(name, generics, _, bound) => {
                self.expression(name);
                if let Some(generics) = generics {
                    self.expressions(generics);
                }
                self.optional(bound.as_deref());
            }
            ExpressionKind::Lambda(lambda) => self.lambda(lambda),
            ExpressionKind::List(items)
            | ExpressionKind::Tuple(items)
            | ExpressionKind::Set(items)
            | ExpressionKind::FormattedString(items) => self.expressions(items),
            ExpressionKind::Array(items, size) => {
                self.expressions(items);
                self.expression(size);
            }
            ExpressionKind::Map(entries) => {
                for (key, value) in entries {
                    self.pair(key, value);
                }
            }
            ExpressionKind::Match(subject, branches) => {
                self.expression(subject);
                for branch in branches {
                    self.match_branch(branch);
                }
            }
            ExpressionKind::Block(statements, tail) => {
                self.scope(statements, false);
                self.expression(tail);
            }
        }
    }

    fn pair(&mut self, left: &'a Expression, right: &'a Expression) {
        self.expression(left);
        self.expression(right);
    }

    fn optional(&mut self, expression: Option<&'a Expression>) {
        if let Some(expression) = expression {
            self.expression(expression);
        }
    }

    fn left_hand_side(&mut self, target: &'a LeftHandSideExpression) {
        match target {
            LeftHandSideExpression::Identifier(target)
            | LeftHandSideExpression::Member(target)
            | LeftHandSideExpression::Index(target) => self.expression(target),
        }
    }

    fn match_branch(&mut self, branch: &'a MatchBranch) {
        if let Some(guard) = &branch.guard {
            self.expression(guard);
        }
        self.body(&branch.body);
    }
}

/// The names a file puts in reach of another file that imports it.
///
/// Only the top level is read, and only what is not private: a `use` reaches a
/// module's own declarations and nothing nested inside them.
pub(crate) fn exported_names(statements: &[Statement]) -> HashSet<String> {
    let mut names = HashSet::new();
    for statement in statements {
        collect_exported(statement, &mut names);
    }
    names
}

fn collect_exported(statement: &Statement, names: &mut HashSet<String>) {
    match &statement.node {
        // The parser groups consecutive top-level statements under a block, so
        // a scan of the body alone would miss whatever landed inside one.
        StatementKind::Block(statements) => {
            for nested in statements {
                collect_exported(nested, names);
            }
        }
        StatementKind::FunctionDeclaration(declaration) => {
            insert_exported(
                Some(declaration.name.as_str()),
                &declaration.properties.visibility,
                names,
            );
        }
        StatementKind::IntrinsicFunctionDeclaration(name, _, _, _, visibility) => {
            insert_exported(Some(name.as_str()), visibility, names);
        }
        StatementKind::Class(class) => {
            insert_exported(declared_name(Some(&class.name)), &class.visibility, names);
        }
        StatementKind::Enum(name, _, variants, _, visibility, _) => {
            insert_exported(declared_name(Some(name)), visibility, names);
            for variant in variants {
                insert_exported(declared_name(Some(variant)), visibility, names);
            }
        }
        StatementKind::Struct(name, _, _, _, visibility, _)
        | StatementKind::Trait(name, _, _, _, visibility) => {
            insert_exported(declared_name(Some(name)), visibility, names);
        }
        StatementKind::Type(parts, visibility) => {
            insert_exported(declared_name(parts.first()), visibility, names);
        }
        StatementKind::Variable(declarations, visibility) => {
            for declaration in declarations {
                insert_exported(Some(declaration.name.as_str()), visibility, names);
            }
        }
        // A runtime binding is private to its declaring module by definition,
        // and the rest declare no name at all.
        StatementKind::RuntimeFunctionDeclaration(..)
        | StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Expression(_)
        | StatementKind::If(..)
        | StatementKind::While(..)
        | StatementKind::For(..)
        | StatementKind::Forall { .. }
        | StatementKind::GpuFrame(..)
        | StatementKind::GpuFrameBlock(_)
        | StatementKind::Return(_)
        | StatementKind::Use(_, _) => {}
    }
}

fn insert_exported(name: Option<&str>, visibility: &MemberVisibility, names: &mut HashSet<String>) {
    if matches!(visibility, MemberVisibility::Private) {
        return;
    }
    if let Some(name) = name {
        names.insert(name.to_string());
    }
}

/// The name a declaration head introduces, when the head is a plain name.
///
/// A head this cannot read declares nothing the checks can ask about, and is
/// left alone rather than guessed at.
fn declared_name(head: Option<&Expression>) -> Option<&str> {
    let head = head?;
    if let ExpressionKind::Identifier(name, _) = &head.node {
        return Some(name);
    }
    if let ExpressionKind::GenericType(name, _, _) = &head.node {
        return declared_name(Some(name));
    }
    if let ExpressionKind::TypeDeclaration(name, _, _, _) = &head.node {
        return declared_name(Some(name));
    }
    if let ExpressionKind::EnumValue(name, _) = &head.node {
        return declared_name(Some(name));
    }
    None
}

/// Where a report about a function itself points: at the declared name when the
/// parser recorded it, and at the declaration otherwise.
fn name_span(
    declaration: &FunctionDeclarationData,
    statement: &Statement,
) -> crate::error::syntax::Span {
    if declaration.name_span.is_empty() {
        statement.span
    } else {
        declaration.name_span
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn parse(source: &str) -> crate::ast::Program {
        let mut lexer = Lexer::new(source);
        let mut parser = Parser::new(&mut lexer, source);
        match parser.parse() {
            Ok(program) => program,
            Err(error) => panic!("the fixture should parse: {:?}", error),
        }
    }

    fn exported(source: &str) -> Vec<String> {
        let program = parse(source);
        let mut names: Vec<String> = exported_names(&program.body).into_iter().collect();
        names.sort();
        names
    }

    #[test]
    fn test_a_public_declaration_is_exported() {
        assert_eq!(
            exported("public fn visible() int\n    1\n"),
            vec!["visible"]
        );
    }

    #[test]
    fn test_a_private_declaration_is_not_exported() {
        assert!(exported("private fn hidden() int\n    1\n").is_empty());
    }

    #[test]
    fn test_a_module_scope_binding_is_exported() {
        assert_eq!(exported("const LIMIT = 10\n"), vec!["LIMIT"]);
    }

    #[test]
    fn test_an_enum_exports_its_own_name_and_its_variants() {
        assert_eq!(
            exported("enum Color\n    Red\n    Green\n"),
            vec!["Color", "Green", "Red"]
        );
    }

    #[test]
    fn test_a_nested_declaration_is_not_exported() {
        assert_eq!(
            exported("public fn outer() int\n    let inner = 1\n    inner\n"),
            vec!["outer"]
        );
    }

    #[test]
    fn test_a_function_body_opens_a_scope_of_its_own() {
        let program = parse("fn main()\n    let value = 1\n    println(f\"{value}\")\n");
        let contents = contents(&program.body);
        assert!(
            contents.scopes.iter().any(|scope| !scope.is_module),
            "the body of a function is a scope, and it is not the module's"
        );
        assert_eq!(contents.functions.len(), 1);
    }
}
