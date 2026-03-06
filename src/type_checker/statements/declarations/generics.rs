// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Statement type checking for the type checker.
//!
//! This module implements type checking for all statement kinds in Miri.
//! The main entry point is [`TypeChecker::check_statement`], which validates
//! statements and registers type information in the context.
//!
//! # Supported Statements
//!
//! ## Declarations
//! - Variable declarations: `let x = 1`, `var y: int = 2`
//! - Function declarations with generics and return type validation
//! - Struct, enum, class, and trait definitions
//! - Type aliases
//!
//! ## Control Flow
//! - If/else statements with condition type checking
//! - While loops (including forever loops)
//! - For loops with iterator type inference
//! - Match statements with exhaustiveness checking
//! - Return statements with type compatibility validation
//!
//! ## Expressions
//! - Expression statements (side effects)
//! - Assignment validation
//!
//! ## Type Definitions
//! - Structs with fields and generic parameters
//! - Enums with variants and associated values
//! - Classes with fields, methods, and inheritance
//! - Traits with method signatures
//!
//! # Return Type Analysis
//!
//! The module includes return status analysis (`check_returns`) to determine:
//! - Whether all code paths return a value
//! - Implicit vs explicit returns
//! - Return type compatibility

use crate::ast::*;
use crate::type_checker::context::{Context, GenericDefinition};
use crate::type_checker::TypeChecker;

impl TypeChecker {
    pub(crate) fn extract_generic_definitions(
        &mut self,
        generics: &[Expression],
        context: &mut Context,
    ) -> Vec<GenericDefinition> {
        let mut result = Vec::with_capacity(generics.len());
        for gen_expr in generics {
            if let ExpressionKind::GenericType(name_expr, constraint_expr, kind) = &gen_expr.node {
                if let Ok(gen_name) = self.extract_type_name(name_expr) {
                    let constraint = constraint_expr
                        .as_ref()
                        .map(|c| self.resolve_type_expression(c, context));
                    result.push(GenericDefinition {
                        name: gen_name.to_string(),
                        constraint,
                        kind: kind.clone(),
                    });
                }
            }
        }
        result
    }
}
