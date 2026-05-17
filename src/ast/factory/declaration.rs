// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::primitives::{empty_statement, identifier};
use super::{expr, stmt};
use crate::ast::common::{FunctionProperties, MemberVisibility, Parameter, RuntimeKind};
use crate::ast::expression::{Expression, ExpressionKind, LambdaData};
use crate::ast::statement::{ClassData, FunctionDeclarationData, Statement, StatementKind};

/// Creates a function parameter.
pub fn parameter(
    name: String,
    typ: Expression,
    guard: Option<Box<Expression>>,
    default_value: Option<Box<Expression>>,
) -> Parameter {
    Parameter {
        name,
        typ: Box::new(typ),
        guard,
        default_value,
        is_out: false,
    }
}

/// Creates an `out` function parameter.
pub fn out_parameter(
    name: String,
    typ: Expression,
    guard: Option<Box<Expression>>,
    default_value: Option<Box<Expression>>,
) -> Parameter {
    Parameter {
        name,
        typ: Box::new(typ),
        guard,
        default_value,
        is_out: true,
    }
}

/// Creates an enum declaration statement.
pub fn enum_statement(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    values: Vec<Expression>,
    methods: Vec<Statement>,
    visibility: MemberVisibility,
    must_use: bool,
) -> Statement {
    stmt(StatementKind::Enum(
        Box::new(name),
        generic_types,
        values,
        methods,
        visibility,
        must_use,
    ))
}

/// Creates an enum value expression (variant).
pub fn enum_value_expression(name: Expression, types: Vec<Expression>) -> Expression {
    expr(ExpressionKind::EnumValue(Box::new(name), types))
}

/// Creates an enum value (variant) from a string name.
pub fn enum_value(name: &str, types: Vec<Expression>) -> Expression {
    enum_value_expression(identifier(name), types)
}

/// Creates a struct declaration statement.
pub fn struct_statement(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    members: Vec<Expression>,
    methods: Vec<Statement>,
    visibility: MemberVisibility,
) -> Statement {
    stmt(StatementKind::Struct(
        Box::new(name),
        generic_types,
        members,
        methods,
        visibility,
    ))
}

/// Creates a struct member expression (name and type pair).
pub fn struct_member_expression(name: Expression, typ: Expression) -> Expression {
    expr(ExpressionKind::StructMember(Box::new(name), Box::new(typ)))
}

/// Creates a struct member from a string name and type expression.
pub fn struct_member(name: &str, typ: Expression) -> Expression {
    struct_member_expression(identifier(name), typ)
}

/// Creates a class declaration statement.
pub fn class_statement(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    base_class: Option<Box<Expression>>,
    traits: Vec<Expression>,
    body: Vec<Statement>,
    visibility: MemberVisibility,
) -> Statement {
    stmt(StatementKind::Class(Box::new(ClassData {
        name: Box::new(name),
        generics: generic_types,
        base_class,
        traits,
        body,
        visibility,
        is_abstract: false,
    })))
}

/// Creates an abstract class declaration statement.
pub fn abstract_class_statement(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    base_class: Option<Box<Expression>>,
    traits: Vec<Expression>,
    body: Vec<Statement>,
    visibility: MemberVisibility,
) -> Statement {
    stmt(StatementKind::Class(Box::new(ClassData {
        name: Box::new(name),
        generics: generic_types,
        base_class,
        traits,
        body,
        visibility,
        is_abstract: true,
    })))
}

/// Creates a class declaration from string name.
pub fn class_decl(
    name: &str,
    generic_types: Option<Vec<Expression>>,
    base_class: Option<&str>,
    traits: Vec<&str>,
    body: Vec<Statement>,
    visibility: MemberVisibility,
) -> Statement {
    class_statement(
        identifier(name),
        generic_types,
        base_class.map(|s| Box::new(identifier(s))),
        traits.into_iter().map(identifier).collect(),
        body,
        visibility,
    )
}

/// Creates a trait declaration statement.
pub fn trait_statement(
    name: Expression,
    generic_types: Option<Vec<Expression>>,
    parent_traits: Vec<Expression>,
    body: Vec<Statement>,
    visibility: MemberVisibility,
) -> Statement {
    stmt(StatementKind::Trait(
        Box::new(name),
        generic_types,
        parent_traits,
        body,
        visibility,
    ))
}

/// Creates a trait declaration from string name.
pub fn trait_decl(
    name: &str,
    generic_types: Option<Vec<Expression>>,
    parent_traits: Vec<&str>,
    body: Vec<Statement>,
    visibility: MemberVisibility,
) -> Statement {
    trait_statement(
        identifier(name),
        generic_types,
        parent_traits.into_iter().map(identifier).collect(),
        body,
        visibility,
    )
}

/// Represents a function builder, used to create functions with a more readable syntax.
pub struct FunctionBuilder {
    name: String,
    generic_types: Option<Vec<Expression>>,
    parameters: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
    properties: FunctionProperties,
}

impl FunctionBuilder {
    /// Creates a new function builder with the given name.
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            generic_types: None,
            parameters: vec![],
            return_type: None,
            properties: FunctionProperties {
                is_async: false,
                is_parallel: false,
                is_gpu: false,
                visibility: MemberVisibility::Public,
            },
        }
    }

    /// Sets the generic type parameters.
    pub fn generics(mut self, generics: Vec<Expression>) -> Self {
        self.generic_types = Some(generics);
        self
    }

    /// Sets the function parameters.
    pub fn params(mut self, params: Vec<Parameter>) -> Self {
        self.parameters = params;
        self
    }

    /// Sets the function properties (async, parallel, gpu, visibility).
    pub fn properties(mut self, properties: FunctionProperties) -> Self {
        self.properties = properties;
        self
    }

    /// Sets the return type.
    pub fn return_type(mut self, ret_type: Expression) -> Self {
        self.return_type = Some(Box::new(ret_type));
        self
    }

    /// Marks the function as async.
    pub fn set_async(mut self) -> Self {
        self.properties.is_async = true;
        self
    }

    /// Marks the function as parallel.
    pub fn set_parallel(mut self) -> Self {
        self.properties.is_parallel = true;
        self
    }

    /// Marks the function as a GPU kernel.
    pub fn set_gpu(mut self) -> Self {
        self.properties.is_gpu = true;
        self
    }

    /// Sets visibility to private.
    pub fn set_private(mut self) -> Self {
        self.properties.visibility = MemberVisibility::Private;
        self
    }

    /// Sets visibility to protected.
    pub fn set_protected(mut self) -> Self {
        self.properties.visibility = MemberVisibility::Protected;
        self
    }

    /// Builds a function declaration statement with the given body.
    pub fn build(self, body: Statement) -> Statement {
        stmt(StatementKind::FunctionDeclaration(Box::new(
            FunctionDeclarationData {
                name: self.name,
                generics: self.generic_types,
                params: self.parameters,
                return_type: self.return_type,
                body: Some(Box::new(body)),
                properties: self.properties,
            },
        )))
    }

    /// Builds an abstract function declaration (no body).
    pub fn build_abstract(self) -> Statement {
        stmt(StatementKind::FunctionDeclaration(Box::new(
            FunctionDeclarationData {
                name: self.name,
                generics: self.generic_types,
                params: self.parameters,
                return_type: self.return_type,
                body: None,
                properties: self.properties,
            },
        )))
    }

    /// Builds a function declaration with an empty body.
    pub fn build_empty_body(self) -> Statement {
        self.build(empty_statement())
    }

    /// Builds a lambda expression with the given body.
    pub fn build_lambda(self, body: Statement) -> Expression {
        expr(ExpressionKind::Lambda(Box::new(LambdaData {
            generics: self.generic_types,
            params: self.parameters,
            return_type: self.return_type,
            body: Box::new(body),
            properties: self.properties,
        })))
    }

    /// Builds a lambda expression with an empty body.
    pub fn build_lambda_empty_body(self) -> Expression {
        self.build_lambda(empty_statement())
    }
}

/// Creates a function builder.
pub fn func(name: &str) -> FunctionBuilder {
    FunctionBuilder::new(name)
}

/// Creates a function declaration statement.
pub fn function_declaration(
    name: &str,
    generic_types: Option<Vec<Expression>>,
    parameters: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
    body: Statement,
    properties: FunctionProperties,
) -> Statement {
    stmt(StatementKind::FunctionDeclaration(Box::new(
        FunctionDeclarationData {
            name: name.into(),
            generics: generic_types,
            params: parameters,
            return_type,
            body: Some(Box::new(body)),
            properties,
        },
    )))
}

/// Creates an abstract function declaration (no body).
pub fn abstract_function_declaration(
    name: &str,
    generic_types: Option<Vec<Expression>>,
    parameters: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
    properties: FunctionProperties,
) -> Statement {
    stmt(StatementKind::FunctionDeclaration(Box::new(
        FunctionDeclarationData {
            name: name.into(),
            generics: generic_types,
            params: parameters,
            return_type,
            body: None,
            properties,
        },
    )))
}

/// Creates a runtime function declaration (extern binding to a runtime library).
pub fn runtime_function_declaration(
    runtime: RuntimeKind,
    name: &str,
    parameters: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
) -> Statement {
    stmt(StatementKind::RuntimeFunctionDeclaration(
        runtime,
        name.into(),
        parameters,
        return_type,
    ))
}

/// Creates an intrinsic function declaration (compiler-implemented function).
pub fn intrinsic_function_declaration(
    name: &str,
    generics: Option<Vec<Expression>>,
    parameters: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
    visibility: MemberVisibility,
) -> Statement {
    stmt(StatementKind::IntrinsicFunctionDeclaration(
        name.into(),
        generics,
        parameters,
        return_type,
        visibility,
    ))
}

/// Creates a lambda function builder.
pub fn lambda() -> FunctionBuilder {
    FunctionBuilder::new("")
}

/// Creates a lambda function expression.
pub fn lambda_expression(
    generic_types: Option<Vec<Expression>>,
    parameters: Vec<Parameter>,
    return_type: Option<Box<Expression>>,
    body: Statement,
    properties: FunctionProperties,
) -> Expression {
    expr(ExpressionKind::Lambda(Box::new(LambdaData {
        generics: generic_types,
        params: parameters,
        return_type,
        body: Box::new(body),
        properties,
    })))
}
