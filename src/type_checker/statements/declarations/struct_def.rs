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
    Context, GenericDefinition, StructDefinition, SymbolInfo, TypeDefinition,
};
use crate::type_checker::TypeChecker;

/// Returns true if a function declaration statement is `fn drop(self)`.
fn is_drop_method(stmt: &Statement) -> bool {
    if let StatementKind::FunctionDeclaration(decl) = &stmt.node {
        if decl.name == "drop" && decl.params.len() == 1 && decl.params[0].name == "self" {
            return true;
        }
    }
    false
}

impl TypeChecker {
    pub(crate) fn check_struct(
        &mut self,
        name_expr: &Expression,
        generics: &Option<Vec<Expression>>,
        fields: &[Expression],
        methods: &[Statement],
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        let name = if let ExpressionKind::Identifier(n, _) = &name_expr.node {
            n.clone()
        } else {
            self.report_error("Invalid struct name".to_string(), name_expr.span);
            return;
        };

        // Check for duplicate type definitions
        if let Some(existing) = self.global_type_definitions.get(&name) {
            let is_placeholder = match existing {
                TypeDefinition::Struct(def) => def.fields.is_empty(),
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

        let capacity = generics.as_ref().map(|g| g.len()).unwrap_or(0);
        let mut generic_defs = Vec::with_capacity(capacity);
        context.enter_scope();
        if let Some(gens) = generics {
            self.define_generics(gens, context);
            for gen in gens {
                if let ExpressionKind::GenericType(name_expr, constraint_expr, kind) = &gen.node {
                    if let ExpressionKind::Identifier(n, _) = &name_expr.node {
                        let constraint_type = constraint_expr
                            .as_ref()
                            .map(|c| self.resolve_type_expression(c, context));
                        generic_defs.push(GenericDefinition {
                            name: n.clone(),
                            constraint: constraint_type,
                            kind: kind.clone(),
                        });
                    }
                }
            }
        }

        let mut fields_vec = Vec::with_capacity(fields.len());
        for field in fields {
            if let ExpressionKind::StructMember(field_name_expr, field_type_expr) = &field.node {
                if let ExpressionKind::Identifier(field_name, _) = &field_name_expr.node {
                    let field_type = self.resolve_type_expression(field_type_expr, context);
                    fields_vec.push((field_name.clone(), field_type, MemberVisibility::Public));
                } else {
                    self.report_error(
                        "Invalid struct field name".to_string(),
                        field_name_expr.span,
                    );
                }
            } else {
                self.report_error("Invalid struct field definition".to_string(), field.span);
            }
        }

        context.exit_scope();

        // Detect infinite recursive struct types: a struct that contains itself
        // (directly or indirectly) without going through an optional type would
        // have infinite size and cannot be instantiated.
        for (field_name, field_type, _) in &fields_vec {
            if self.is_infinite_recursive_type(&name, &field_type.kind) {
                self.report_error(
                    format!(
                        "Infinite recursive type: field '{}' of struct '{}' contains '{}' without indirection",
                        field_name, name, name
                    ),
                    name_expr.span,
                );
                return;
            }
        }

        let has_drop = methods.iter().any(is_drop_method);

        let struct_def = StructDefinition {
            fields: fields_vec,
            generics: if generic_defs.is_empty() {
                None
            } else {
                Some(generic_defs)
            },
            has_drop,
            module: self.current_module.clone(),
        };

        context.define_type(name.clone(), TypeDefinition::Struct(struct_def.clone()));
        if context.scopes.len() == 1 {
            self.register_type_definition(name.clone(), TypeDefinition::Struct(struct_def));
        }

        // Define constructor/type symbol
        // The type of the struct name identifier is Meta(Custom(name))
        let struct_type = make_type(TypeKind::Custom(name.clone(), None)); // TODO: Handle generics

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.clone(),
                SymbolInfo::new(
                    make_type(TypeKind::Meta(Box::new(struct_type.clone()))),
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
                make_type(TypeKind::Meta(Box::new(struct_type))),
                false,
                false,
                visibility.clone(),
                self.current_module.clone(),
                None,
            ),
        );
    }
}
