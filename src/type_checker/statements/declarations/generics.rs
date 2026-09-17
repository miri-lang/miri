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

use crate::ast::factory::make_type;
use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::type_checker::context::{Context, GenericDefinition, TypeDefinition};
use crate::type_checker::TypeChecker;

impl TypeChecker {
    /// The type a class names itself by inside its own body: the class at its
    /// own generic parameters (`Tagged<T>`), or the bare name when it declares
    /// none. `self` and `Self` both resolve to it.
    ///
    /// A generic class named bare would be carried into its method signatures
    /// that way, so a call site substituting the receiver's type arguments
    /// would read a `Self` parameter as the class with no arguments — failing
    /// the arity check and matching no instantiation.
    ///
    /// The generic parameters must already be defined in `context`.
    pub(crate) fn own_class_type(
        &self,
        name: &str,
        generics: Option<&[Expression]>,
        context: &Context,
    ) -> Type {
        let type_args: Vec<Expression> = generics
            .unwrap_or_default()
            .iter()
            .filter_map(|generic| self.generic_parameter_type(generic, context))
            .map(|param| self.create_type_expression(param))
            .collect();
        let type_args = (!type_args.is_empty()).then_some(type_args);
        make_type(TypeKind::Custom(name.to_string(), type_args))
    }

    /// The type a declared generic parameter stands for inside its owner's body.
    fn generic_parameter_type(&self, generic: &Expression, context: &Context) -> Option<Type> {
        let ExpressionKind::GenericType(name_expr, _, _) = &generic.node else {
            return None;
        };
        let name = self.extract_type_name(name_expr).ok()?;
        let Some(TypeDefinition::Generic(def)) = context.resolve_type_definition(name) else {
            return None;
        };
        Some(make_type(TypeKind::Generic(
            def.name.clone(),
            def.constraint.clone().map(Box::new),
            def.kind,
        )))
    }

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
                        kind: *kind,
                    });
                }
            }
        }
        result
    }
}
