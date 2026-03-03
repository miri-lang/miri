// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression type inference for the type checker.
//!
//! This module implements type inference for all expression kinds in Miri.
//! The main entry point is [`TypeChecker::infer_expression`], which dispatches
//! to specialized inference methods based on the expression kind.
//!
//! # Supported Expressions
//!
//! ## Literals
//! - Integer, float, string, boolean, and none literals
//!
//! ## Operators
//! - Binary: arithmetic (`+`, `-`, `*`, `/`, `%`), comparison (`<`, `>`, `==`, etc.)
//! - Logical: `and`, `or`
//! - Unary: `-`, `+`, `not`, `~`, `await`
//!
//! ## Collections
//! - Lists: `[1, 2, 3]` → `List<int>`
//! - Maps: `{"a": 1}` → `Map<string, int>`
//! - Sets: `{1, 2, 3}` → `Set<int>`
//! - Tuples: `(1, "a")` → `(int, string)`
//! - Ranges: `1..10` → `Range<int>`
//!
//! ## Access
//! - Member access: `obj.field`
//! - Index access: `list[0]`, `map["key"]`
//!
//! ## Functions
//! - Function calls with generic type inference
//! - Lambda expressions with type inference
//! - Method calls on objects
//!
//! ## Control Flow
//! - Conditional expressions: `x if cond else y`
//! - Match expressions with pattern matching
//!
//! ## Types
//! - Struct instantiation: `Point { x: 1, y: 2 }`
//! - Enum variant construction: `Ok(value)`, `Err(error)`
//! - Generic type instantiation

use crate::ast::factory as ast_factory;
use crate::ast::factory::make_type;
use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::type_checker::context::Context;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    pub(crate) fn infer_literal(&self, lit: &Literal) -> Type {
        match lit {
            Literal::Integer(_) => ast_factory::make_type(TypeKind::Int),
            Literal::Float(f) => match f {
                FloatLiteral::F32(_) => ast_factory::make_type(TypeKind::F32),
                FloatLiteral::F64(_) => ast_factory::make_type(TypeKind::F64),
            },
            Literal::Boolean(_) => ast_factory::make_type(TypeKind::Boolean),
            Literal::String(_) => ast_factory::make_type(TypeKind::String),
            Literal::Symbol(_) => ast_factory::make_type(TypeKind::Symbol),
            Literal::Regex(_) => ast_factory::make_type(TypeKind::Custom("Regex".into(), None)),
            Literal::None => ast_factory::make_type(TypeKind::Option(Box::new(
                ast_factory::make_type(TypeKind::Void),
            ))),
        }
    }

    pub(crate) fn infer_formatted_string(
        &mut self,
        parts: &[Expression],
        context: &mut Context,
    ) -> Type {
        for part in parts {
            self.infer_expression(part, context);
        }
        make_type(TypeKind::String)
    }
}
