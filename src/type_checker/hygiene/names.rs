// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every name a run of statements refers to.
//!
//! The hygiene checks all ask one question — is this name read anywhere? — so
//! they all need the same answer: the set of names that appear in a *reference*
//! position. A reference is anything that reads a name the program declared
//! somewhere else: a variable read, a call target, a member name, a type
//! written in an annotation, a base class, a trait in an `implements` list.
//!
//! Declaration positions are deliberately excluded, because a name that only
//! ever appears where it is introduced is exactly what the checks are looking
//! for. Everything else is included, and where a position could be read either
//! way it is counted as a reference: an over-broad set costs a warning that is
//! never raised, while a set that misses one costs a warning about a name the
//! program really uses.

use crate::ast::common::Parameter;
use crate::ast::expression::{Expression, ExpressionKind, LambdaData, LeftHandSideExpression};
use crate::ast::pattern::{MatchBranch, Pattern};
use crate::ast::statement::{
    ClassData, FunctionDeclarationData, Statement, StatementKind, VariableDeclaration,
};
use crate::ast::types::{
    BuiltinCollectionKind, Type, TypeKind, OPTION_TYPE_NAME, RESULT_TYPE_NAME, STRING_TYPE_NAME,
    TUPLE_TYPE_NAME,
};
use std::collections::HashSet;

/// The names referred to by a run of statements.
#[derive(Debug, Default)]
pub(crate) struct References {
    names: HashSet<String>,
}

impl References {
    /// Collects every name `statements` refers to, at any depth.
    pub(crate) fn of_statements(statements: &[Statement]) -> Self {
        let mut references = Self::default();
        for statement in statements {
            references.statement(statement);
        }
        references
    }

    /// Collects every name a function's body reads, plus the names its own
    /// parameter list reads: a parameter used only as another parameter's
    /// default or guard is read by the declaration even though no statement
    /// mentions it.
    ///
    /// The parameter names themselves are declarations, not references, so they
    /// are not collected — which is what lets this answer whether a parameter is
    /// read at all.
    pub(crate) fn of_function_body(declaration: &FunctionDeclarationData) -> Self {
        let mut references = Self::default();
        if let Some(body) = &declaration.body {
            references.statement(body);
        }
        for parameter in &declaration.params {
            if let Some(guard) = &parameter.guard {
                references.expression(guard);
            }
            if let Some(default_value) = &parameter.default_value {
                references.expression(default_value);
            }
        }
        references
    }

    /// True when `name` is read somewhere in the statements this was built from.
    pub(crate) fn contains(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    fn record(&mut self, name: &str) {
        self.names.insert(name.to_string());
    }

    fn statement(&mut self, statement: &Statement) {
        match &statement.node {
            StatementKind::Empty | StatementKind::Break | StatementKind::Continue => {}
            // An import path names a module and the names taken from it. Both
            // are what the unused-import check is asking about, so counting
            // them here would make every import look used.
            StatementKind::Use(_, _) => {}
            StatementKind::Expression(expression) => self.expression(expression),
            StatementKind::Block(statements) => self.statements(statements),
            StatementKind::Variable(declarations, _) => self.variables(declarations),
            StatementKind::If(condition, then_branch, else_branch, _) => {
                self.expression(condition);
                self.statement(then_branch);
                self.optional_statement(else_branch.as_deref());
            }
            StatementKind::While(condition, body, _) => {
                self.expression(condition);
                self.statement(body);
            }
            StatementKind::For(variables, iterable, body)
            | StatementKind::GpuFrame(variables, iterable, body) => {
                self.loop_over(variables, iterable, body)
            }
            StatementKind::Forall {
                vars,
                iterable,
                body,
                device: _,
            } => self.loop_over(vars, iterable, body),
            StatementKind::GpuFrameBlock(body) => self.statement(body),
            StatementKind::FunctionDeclaration(declaration) => self.function(declaration),
            StatementKind::Return(value) => self.optional_expression(value.as_deref()),
            StatementKind::Type(parts, _) => {
                for part in parts {
                    self.declared_type_expression(part);
                }
            }
            StatementKind::Enum(_, generics, variants, methods, _, _) => {
                self.expression_list(generics.as_deref());
                self.expressions(variants);
                self.statements(methods);
            }
            StatementKind::Struct(_, generics, fields, methods, _, traits) => {
                self.expression_list(generics.as_deref());
                self.expressions(fields);
                self.statements(methods);
                self.expressions(traits);
            }
            StatementKind::Class(class) => self.class(class),
            StatementKind::Trait(_, generics, parents, body, _) => {
                self.expression_list(generics.as_deref());
                self.expressions(parents);
                self.statements(body);
            }
            StatementKind::RuntimeFunctionDeclaration(_, _, parameters, return_type) => {
                self.signature(None, parameters, return_type.as_deref())
            }
            StatementKind::IntrinsicFunctionDeclaration(_, generics, params, return_type, _) => {
                self.signature(generics.as_deref(), params, return_type.as_deref())
            }
        }
    }

    /// The three parts of a loop that read names: what it binds, what it walks,
    /// and what it does.
    fn loop_over(
        &mut self,
        variables: &[VariableDeclaration],
        iterable: &Expression,
        body: &Statement,
    ) {
        self.variables(variables);
        self.expression(iterable);
        self.statement(body);
    }

    /// A declaration that names types without carrying a body.
    fn signature(
        &mut self,
        generics: Option<&[Expression]>,
        parameters: &[Parameter],
        return_type: Option<&Expression>,
    ) {
        self.expression_list(generics);
        self.parameters(parameters);
        self.optional_expression(return_type);
    }

    fn optional_expression(&mut self, expression: Option<&Expression>) {
        if let Some(expression) = expression {
            self.expression(expression);
        }
    }

    fn optional_statement(&mut self, statement: Option<&Statement>) {
        if let Some(statement) = statement {
            self.statement(statement);
        }
    }

    fn statements(&mut self, statements: &[Statement]) {
        for statement in statements {
            self.statement(statement);
        }
    }

    fn class(&mut self, class: &ClassData) {
        self.expression_list(class.generics.as_deref());
        if let Some(base_class) = &class.base_class {
            self.expression(base_class);
        }
        self.expressions(&class.traits);
        self.statements(&class.body);
    }

    fn function(&mut self, declaration: &FunctionDeclarationData) {
        self.expression_list(declaration.generics.as_deref());
        self.parameters(&declaration.params);
        if let Some(return_type) = &declaration.return_type {
            self.expression(return_type);
        }
        if let Some(body) = &declaration.body {
            self.statement(body);
        }
    }

    fn parameters(&mut self, parameters: &[Parameter]) {
        for parameter in parameters {
            self.expression(&parameter.typ);
            if let Some(guard) = &parameter.guard {
                self.expression(guard);
            }
            if let Some(default_value) = &parameter.default_value {
                self.expression(default_value);
            }
        }
    }

    fn variables(&mut self, declarations: &[VariableDeclaration]) {
        for declaration in declarations {
            if let Some(typ) = &declaration.typ {
                self.expression(typ);
            }
            if let Some(initializer) = &declaration.initializer {
                self.expression(initializer);
            }
        }
    }

    /// A declaration whose head is the name being introduced: the head is
    /// skipped and everything it is declared against is still a reference.
    fn declared_type_expression(&mut self, expression: &Expression) {
        if let ExpressionKind::TypeDeclaration(_, generics, _, bound) = &expression.node {
            self.expression_list(generics.as_deref());
            if let Some(bound) = bound {
                self.expression(bound);
            }
            return;
        }
        if let ExpressionKind::GenericType(_, arguments, _) = &expression.node {
            if let Some(arguments) = arguments {
                self.expression(arguments);
            }
            return;
        }
        if matches!(expression.node, ExpressionKind::Identifier(_, _)) {
            return;
        }
        self.expression(expression);
    }

    fn expressions(&mut self, expressions: &[Expression]) {
        for expression in expressions {
            self.expression(expression);
        }
    }

    fn expression_list(&mut self, expressions: Option<&[Expression]>) {
        if let Some(expressions) = expressions {
            self.expressions(expressions);
        }
    }

    fn expression(&mut self, expression: &Expression) {
        match &expression.node {
            ExpressionKind::Literal(_) | ExpressionKind::Super => {}
            ExpressionKind::Identifier(name, qualifier) => {
                self.record(name);
                if let Some(qualifier) = qualifier {
                    self.record(qualifier);
                }
            }
            ExpressionKind::Binary(left, _, right)
            | ExpressionKind::Logical(left, _, right)
            | ExpressionKind::Member(left, right)
            | ExpressionKind::Index(left, right)
            | ExpressionKind::Cast(left, right) => self.pair(left, right),
            ExpressionKind::Array(items, size) => {
                self.expressions(items);
                self.expression(size);
            }
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
                self.optional_expression(else_value.as_deref());
            }
            ExpressionKind::Range(start, end, _) => {
                self.expression(start);
                self.optional_expression(end.as_deref());
            }
            ExpressionKind::Call(target, arguments) => {
                self.expression(target);
                self.expressions(arguments);
            }
            // An import path reaches here only as the path of a `use`, which
            // the statement walker never descends into.
            ExpressionKind::ImportPath(_, _) => {}
            ExpressionKind::Type(typ, _) => self.typ(typ),
            ExpressionKind::GenericType(name, arguments, _) => {
                self.expression(name);
                self.optional_expression(arguments.as_deref());
            }
            ExpressionKind::TypeDeclaration(name, generics, _, bound) => {
                self.expression(name);
                self.expression_list(generics.as_deref());
                self.optional_expression(bound.as_deref());
            }
            ExpressionKind::EnumValue(path, payload) => {
                self.expression(path);
                self.expressions(payload);
            }
            ExpressionKind::StructMember(_, typ) => self.expression(typ),
            ExpressionKind::Lambda(lambda) => self.lambda(lambda),
            ExpressionKind::List(items)
            | ExpressionKind::Tuple(items)
            | ExpressionKind::Set(items)
            | ExpressionKind::FormattedString(items) => self.expressions(items),
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
                self.statements(statements);
                self.expression(tail);
            }
        }
    }

    fn pair(&mut self, left: &Expression, right: &Expression) {
        self.expression(left);
        self.expression(right);
    }

    fn left_hand_side(&mut self, target: &LeftHandSideExpression) {
        match target {
            LeftHandSideExpression::Identifier(expression)
            | LeftHandSideExpression::Member(expression)
            | LeftHandSideExpression::Index(expression) => self.expression(expression),
        }
    }

    fn lambda(&mut self, lambda: &LambdaData) {
        self.expression_list(lambda.generics.as_deref());
        self.parameters(&lambda.params);
        if let Some(return_type) = &lambda.return_type {
            self.expression(return_type);
        }
        self.statement(&lambda.body);
    }

    fn match_branch(&mut self, branch: &MatchBranch) {
        for pattern in &branch.patterns {
            self.pattern(pattern);
        }
        if let Some(guard) = &branch.guard {
            self.expression(guard);
        }
        self.statement(&branch.body);
    }

    /// A bare identifier in a pattern is either a binding or the name of a
    /// variant, and nothing at this layer tells the two apart. It is recorded,
    /// which costs a warning that is never raised rather than one that is wrong.
    fn pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Literal(_) | Pattern::Regex(_) | Pattern::Default => {}
            Pattern::Identifier(name) => self.record(name),
            Pattern::Tuple(patterns) => {
                for pattern in patterns {
                    self.pattern(pattern);
                }
            }
            Pattern::Member(base, name) => {
                self.pattern(base);
                self.record(name);
            }
            Pattern::EnumVariant(path, bindings) => {
                self.pattern(path);
                for binding in bindings {
                    self.pattern(binding);
                }
            }
        }
    }

    /// A type written in the source names a class as surely as a call does, and
    /// several of those classes have a type kind of their own rather than a
    /// custom name — a `String` parameter is what makes `use system.string`
    /// used. The canonical spelling comes from the type layer, which is the one
    /// place that holds it.
    fn typ(&mut self, typ: &Type) {
        match &typ.kind {
            TypeKind::String => self.record(STRING_TYPE_NAME),
            TypeKind::Int
            | TypeKind::I8
            | TypeKind::I16
            | TypeKind::I32
            | TypeKind::I64
            | TypeKind::I128
            | TypeKind::U8
            | TypeKind::U16
            | TypeKind::U32
            | TypeKind::U64
            | TypeKind::U128
            | TypeKind::Float
            | TypeKind::F16
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::Void
            | TypeKind::Error => {}
            TypeKind::List(element) => {
                self.record(BuiltinCollectionKind::List.name());
                self.expression(element);
            }
            TypeKind::Set(element) => {
                self.record(BuiltinCollectionKind::Set.name());
                self.expression(element);
            }
            TypeKind::Future(element) => self.expression(element),
            TypeKind::Array(element, size) => {
                self.record(BuiltinCollectionKind::Array.name());
                self.expression(element);
                self.expression(size);
            }
            TypeKind::Map(key, value) => {
                self.record(BuiltinCollectionKind::Map.name());
                self.expression(key);
                self.expression(value);
            }
            TypeKind::Result(ok, error) => {
                self.record(RESULT_TYPE_NAME);
                self.expression(ok);
                self.expression(error);
            }
            TypeKind::Tuple(elements) => {
                self.record(TUPLE_TYPE_NAME);
                self.expressions(elements);
            }
            TypeKind::Function(function) => {
                self.expression_list(function.generics.as_deref());
                self.parameters(&function.params);
                if let Some(return_type) = &function.return_type {
                    self.expression(return_type);
                }
            }
            TypeKind::Generic(name, bound, _) => {
                self.record(name);
                if let Some(bound) = bound {
                    self.typ(bound);
                }
            }
            TypeKind::Custom(name, arguments) => {
                self.record(name);
                self.expression_list(arguments.as_deref());
            }
            TypeKind::Option(inner) => {
                self.record(OPTION_TYPE_NAME);
                self.typ(inner);
            }
            TypeKind::Meta(inner) | TypeKind::Linear(inner) => self.typ(inner),
        }
    }
}
