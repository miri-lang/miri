// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::expression::Expression;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum MemberVisibility {
    #[default]
    Public,
    Protected,
    Private,
}

/// Represents the properties of a function declaration
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct FunctionProperties {
    pub is_async: bool,
    pub is_parallel: bool,
    pub is_gpu: bool,
    pub visibility: MemberVisibility,
}

/// Represents a parameter in a function declaration
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Parameter {
    pub name: String,
    pub typ: Box<Expression>,
    pub guard: Option<Box<Expression>>,
    pub default_value: Option<Box<Expression>>,
}
