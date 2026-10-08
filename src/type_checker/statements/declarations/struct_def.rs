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
use crate::ast::statement::DROP_HOOK_NAME;
use crate::ast::types::TypeKind;
use crate::ast::*;
use crate::diagnostics::DiagnosticCode;
use crate::type_checker::context::{
    Context, GenericDefinition, StructDefinition, SymbolInfo, TypeDefinition,
};
use crate::type_checker::TypeChecker;

impl TypeChecker {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_struct(
        &mut self,
        name_expr: &Expression,
        generics: &Option<Vec<Expression>>,
        fields: &[Expression],
        methods: &[Statement],
        visibility: &MemberVisibility,
        traits: &[Expression],
        context: &mut Context,
    ) {
        let Some(name) = self.extract_struct_name(name_expr) else {
            return;
        };
        if !self.check_struct_not_duplicate(&name, name_expr) {
            return;
        }

        context.enter_scope();
        let generic_defs = self.collect_struct_generics(generics, context);
        let fields_vec = self.collect_struct_fields(fields, context);
        context.exit_scope();

        if !self.validate_struct_field_types(&name, &fields_vec, name_expr) {
            return;
        }

        // A struct holds data and nothing else: behavior, traits and drop
        // hooks belong on a class.
        self.reject_struct_traits(&name, traits);
        self.reject_struct_methods(&name, methods);

        let struct_def = StructDefinition {
            fields: fields_vec,
            generics: if generic_defs.is_empty() {
                None
            } else {
                Some(generic_defs)
            },
            module: self.modules.current_module.clone(),
        };

        self.register_struct_definition(&name, struct_def, visibility, context);
    }

    /// Reports an error for every trait a struct lists after `implements`. A
    /// struct is data: it declares no methods for a trait to require or
    /// dispatch to, so a trait it named would only promise what it cannot do.
    fn reject_struct_traits(&mut self, struct_name: &str, traits: &[Expression]) {
        for trait_expr in traits {
            let trait_name = match &trait_expr.node {
                ExpressionKind::Identifier(name, _) => name.clone(),
                ExpressionKind::Type(ty, _) => ty.to_string(),
                _ => trait_expr.node.to_string(),
            };
            self.report_error_with_help(
                DiagnosticCode::TypStructDefinition,
                format!(
                    "Struct '{struct_name}' cannot implement trait '{trait_name}': a struct \
                     holds data only"
                ),
                trait_expr.span,
                format!("make '{struct_name}' a class to implement '{trait_name}'"),
            );
        }
    }

    /// Reports an error for every method a struct declares, `drop` included.
    /// A struct is data: methods and a drop hook belong on a class, and a
    /// function over a struct can take it as a parameter.
    fn reject_struct_methods(&mut self, struct_name: &str, methods: &[Statement]) {
        for method in methods {
            let StatementKind::FunctionDeclaration(decl) = &method.node else {
                continue;
            };
            let help = if decl.name == DROP_HOOK_NAME {
                format!(
                    "a type that runs code when it is released is a resource: make \
                     '{struct_name}' a class to give it a drop hook"
                )
            } else {
                format!(
                    "make '{struct_name}' a class to give it methods, or write a function \
                     that takes the struct"
                )
            };
            self.report_error_with_help(
                DiagnosticCode::TypStructDefinition,
                format!(
                    "Struct '{struct_name}' cannot define method '{}': a struct holds data only",
                    decl.name
                ),
                method.span,
                help,
            );
        }
    }

    fn extract_struct_name(&mut self, name_expr: &Expression) -> Option<String> {
        if let ExpressionKind::Identifier(n, _) = &name_expr.node {
            Some(n.clone())
        } else {
            self.report_error(
                DiagnosticCode::TypStructDefinition,
                "Invalid struct name".to_string(),
                name_expr.span,
            );
            None
        }
    }

    /// Accepts `name` when no type holds it yet or only the struct's own
    /// shell does, and claims the shell so a second declaration of the name
    /// is reported as a duplicate.
    fn check_struct_not_duplicate(&mut self, name: &str, name_expr: &Expression) -> bool {
        if let Some(existing) = self.type_table.global_type_definitions.get(name) {
            let is_placeholder = matches!(existing, TypeDefinition::Struct(_))
                && self.modules.pre_registered_types.contains(name);
            if !is_placeholder {
                self.report_error(
                    DiagnosticCode::TypTypeAlreadyDefined,
                    format!("Type '{}' is already defined", name),
                    name_expr.span,
                );
                return false;
            }
        }
        self.modules.pre_registered_types.remove(name);
        true
    }

    fn collect_struct_generics(
        &mut self,
        generics: &Option<Vec<Expression>>,
        context: &mut Context,
    ) -> Vec<GenericDefinition> {
        let capacity = generics.as_ref().map(|g| g.len()).unwrap_or(0);
        let mut generic_defs = Vec::with_capacity(capacity);
        if let Some(gens) = generics {
            self.define_generics(gens, context);
            for gen in gens {
                if let ExpressionKind::GenericType(name_expr, constraint_expr, kind) = &gen.node {
                    if let ExpressionKind::Identifier(n, _) = &name_expr.node {
                        let constraint_type = constraint_expr
                            .as_ref()
                            .map(|c| self.resolve_bound_type(c, context));
                        generic_defs.push(GenericDefinition {
                            name: n.clone(),
                            constraint: constraint_type,
                            kind: *kind,
                        });
                    }
                }
            }
        }
        generic_defs
    }

    fn collect_struct_fields(
        &mut self,
        fields: &[Expression],
        context: &mut Context,
    ) -> Vec<(String, crate::ast::types::Type, MemberVisibility)> {
        let mut fields_vec = Vec::with_capacity(fields.len());
        for field in fields {
            if let ExpressionKind::StructMember(field_name_expr, field_type_expr) = &field.node {
                if let ExpressionKind::Identifier(field_name, _) = &field_name_expr.node {
                    let field_type = self.resolve_type_expression(field_type_expr, context);
                    fields_vec.push((field_name.clone(), field_type, MemberVisibility::Public));
                } else {
                    self.report_error(
                        DiagnosticCode::TypStructDefinition,
                        "Invalid struct field name".to_string(),
                        field_name_expr.span,
                    );
                }
            } else {
                self.report_error(
                    DiagnosticCode::TypStructDefinition,
                    "Invalid struct field definition".to_string(),
                    field.span,
                );
            }
        }
        fields_vec
    }

    fn validate_struct_field_types(
        &mut self,
        name: &str,
        fields_vec: &[(String, crate::ast::types::Type, MemberVisibility)],
        name_expr: &Expression,
    ) -> bool {
        for (field_name, field_type, _) in fields_vec {
            if self.is_infinite_recursive_type(name, &field_type.kind) {
                self.report_error(DiagnosticCode::TypStructDefinition,
                    format!(
                        "Infinite recursive type: field '{}' of struct '{}' contains '{}' without indirection",
                        field_name, name, name
                    ),
                    name_expr.span,
                );
                return false;
            }
        }
        true
    }

    fn register_struct_definition(
        &mut self,
        name: &str,
        struct_def: StructDefinition,
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        context.define_type(name.to_string(), TypeDefinition::Struct(struct_def.clone()));
        if context.scopes.len() == 1 {
            self.register_type_definition(name.to_string(), TypeDefinition::Struct(struct_def));
        }

        let struct_type = make_type(TypeKind::Custom(name.to_string(), None));
        if context.scopes.len() == 1 {
            self.type_table.global_scope.insert(
                name.to_string(),
                SymbolInfo::new(
                    make_type(TypeKind::Meta(Box::new(struct_type.clone()))),
                    false,
                    false,
                    visibility.clone(),
                    self.modules.current_module.clone(),
                    None,
                ),
            );
        }

        context.define(
            name.to_string(),
            SymbolInfo::new(
                make_type(TypeKind::Meta(Box::new(struct_type))),
                false,
                false,
                visibility.clone(),
                self.modules.current_module.clone(),
                None,
            ),
        );
    }
}
