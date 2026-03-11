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
use crate::ast::types::TypeKind;
use crate::ast::*;
use crate::type_checker::context::{
    Context, EnumDefinition, GenericDefinition, SymbolInfo, TypeDefinition,
};
use crate::type_checker::TypeChecker;
use std::collections::BTreeMap;

impl TypeChecker {
    pub(crate) fn check_enum(
        &mut self,
        name_expr: &Expression,
        generics: &Option<Vec<Expression>>,
        variants: &[Expression],
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        let name = if let ExpressionKind::Identifier(n, _) = &name_expr.node {
            n.clone()
        } else {
            self.report_error("Invalid enum name".to_string(), name_expr.span);
            return;
        };

        // Check for duplicate type definitions
        if let Some(existing) = self.global_type_definitions.get(&name) {
            let is_placeholder = match existing {
                TypeDefinition::Enum(def) => def.variants.is_empty(),
                _ => false,
            };

            if !is_placeholder {
                self.report_error(
                    format!("Type '{}' is already defined", name),
                    name_expr.span,
                );
                return;
            }
        }

        // Handle generics
        let mut generic_defs = None;
        if let Some(gens) = generics {
            context.enter_scope();
            self.define_generics(gens, context);

            let mut defs = Vec::with_capacity(gens.len());
            for gen in gens {
                if let ExpressionKind::GenericType(name_expr, constraint, kind) = &gen.node {
                    if let ExpressionKind::Identifier(n, _) = &name_expr.node {
                        let constraint_type = constraint
                            .as_ref()
                            .map(|c| self.resolve_type_expression(c, context));
                        defs.push(GenericDefinition {
                            name: n.clone(),
                            constraint: constraint_type,
                            kind: kind.clone(),
                        });
                    }
                }
            }
            generic_defs = Some(defs);
        }

        let mut variant_map = BTreeMap::new();
        for variant in variants {
            if let ExpressionKind::EnumValue(variant_name_expr, associated_types) = &variant.node {
                if let ExpressionKind::Identifier(variant_name, _) = &variant_name_expr.node {
                    let mut types = Vec::with_capacity(associated_types.len());
                    for ty_expr in associated_types {
                        types.push(self.resolve_type_expression(ty_expr, context));
                    }
                    variant_map.insert(variant_name.clone(), types);
                } else {
                    self.report_error(
                        "Invalid enum variant name".to_string(),
                        variant_name_expr.span,
                    );
                }
            } else {
                self.report_error("Invalid enum variant definition".to_string(), variant.span);
            }
        }

        let enum_def = EnumDefinition {
            variants: variant_map,
            generics: generic_defs.clone(),
        };

        if generics.is_some() {
            context.exit_scope();
        }

        context.define_type(name.clone(), TypeDefinition::Enum(enum_def.clone()));
        if context.scopes.len() == 1 {
            self.global_type_definitions
                .insert(name.clone(), TypeDefinition::Enum(enum_def));
        }

        // Define enum type symbol
        let enum_type = if let Some(defs) = generic_defs {
            let args = defs
                .iter()
                .map(|g| {
                    crate::ast::factory::type_expr_non_null(make_type(TypeKind::Custom(
                        g.name.clone(),
                        None,
                    )))
                })
                .collect();
            make_type(TypeKind::Custom(name.clone(), Some(args)))
        } else {
            make_type(TypeKind::Custom(name.clone(), None))
        };

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.clone(),
                SymbolInfo::new(
                    make_type(TypeKind::Meta(Box::new(enum_type.clone()))),
                    false,
                    false,
                    visibility.clone(),
                    self.current_module.clone(),
                    None,
                ),
            );
        }

        context.define(
            name,
            SymbolInfo::new(
                make_type(TypeKind::Meta(Box::new(enum_type))),
                false,
                false,
                visibility.clone(),
                self.current_module.clone(),
                None,
            ),
        );
    }
}
