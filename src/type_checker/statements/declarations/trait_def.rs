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
use crate::error::syntax::Span;
use crate::type_checker::context::{
    Context, MethodInfo, SymbolInfo, TraitDefinition, TypeDefinition,
};
use crate::type_checker::statements::declarations::FunctionDeclarationInfo;
use crate::type_checker::TypeChecker;
use std::collections::BTreeMap;

impl TypeChecker {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_trait(
        &mut self,
        name_expr: &Expression,
        generics: &Option<Vec<Expression>>,
        parent_traits: &[Expression],
        body: &[Statement],
        visibility: &MemberVisibility,
        context: &mut Context,
        span: Span,
    ) {
        // Extract trait name
        let name = match self.extract_type_name(name_expr) {
            Ok(n) => n.to_string(),
            Err(_) => {
                self.report_error("Invalid trait name".to_string(), name_expr.span);
                return;
            }
        };

        // Check for duplicate type definitions
        if let Some(existing) = self.global_type_definitions.get(&name) {
            let is_placeholder = match existing {
                TypeDefinition::Trait(def) => def.methods.is_empty(),
                _ => false,
            };

            if !is_placeholder {
                self.report_error(format!("Type '{}' is already defined", name), span);
                return;
            }
        }

        // Process generics
        let generic_defs = generics
            .as_ref()
            .map(|gens| self.extract_generic_definitions(gens, context));

        // Validate parent traits exist and are actually traits
        let mut parent_trait_names = Vec::with_capacity(parent_traits.len());
        for trait_expr in parent_traits {
            if let Ok(trait_name) = self.extract_type_name(trait_expr) {
                if !self.is_type_visible(trait_name) {
                    self.report_error(
                        format!("Parent trait '{}' is not defined", trait_name),
                        trait_expr.span,
                    );
                } else if let Some(def) = self.global_type_definitions.get(trait_name) {
                    if !matches!(def, TypeDefinition::Trait(_)) {
                        let kind = match def {
                            TypeDefinition::Class(_) => "a class",
                            TypeDefinition::Enum(_) => "an enum",
                            TypeDefinition::Struct(_) => "a struct",
                            TypeDefinition::Alias(_) => "a type alias",
                            TypeDefinition::Generic(_) => "a generic type",
                            TypeDefinition::Trait(_) => unreachable!(),
                        };
                        self.report_error_with_help(
                            format!("'{}' is not a trait", trait_name),
                            trait_expr.span,
                            format!(
                                "'{}' is {} — only traits can be used with 'extends' in a trait definition",
                                trait_name, kind
                            ),
                        );
                    }
                }
                parent_trait_names.push(trait_name.to_string());
            }
        }

        // Enter trait scope
        context.enter_scope();

        // Set trait context so `Self` resolves inside method signatures
        let trait_type = make_type(TypeKind::Custom(name.clone(), None));
        context.enter_class(name.clone(), None, trait_type);

        // Define generics in scope
        if let Some(gens) = generics {
            self.define_generics(gens, context);
        }

        // Process trait body to collect methods
        let mut methods: BTreeMap<String, MethodInfo> = BTreeMap::new();

        for stmt in body {
            match &stmt.node {
                StatementKind::FunctionDeclaration(decl) => {
                    // Check the function declaration
                    self.check_function_declaration(
                        FunctionDeclarationInfo {
                            name: &decl.name,
                            generics: &decl.generics,
                            params: &decl.params,
                            return_type: &decl.return_type,
                            body: decl.body.as_ref().map(|b| b.as_ref()),
                            properties: &decl.properties,
                        },
                        context,
                    );

                    // Collect method info
                    let return_ty = if let Some(rt_expr) = &decl.return_type {
                        self.resolve_type_expression(rt_expr, context)
                    } else {
                        make_type(TypeKind::Void)
                    };

                    let param_types: Vec<(String, Type)> = decl
                        .params
                        .iter()
                        .map(|p| {
                            (
                                p.name.clone(),
                                self.resolve_type_expression(&p.typ, context),
                            )
                        })
                        .collect();

                    // Trait methods are abstract if they have no body
                    let method_is_abstract = decl.body.is_none();

                    methods.insert(
                        decl.name.clone(),
                        MethodInfo {
                            params: param_types,
                            return_type: return_ty,
                            visibility: decl.properties.visibility.clone(),
                            is_constructor: false,
                            is_abstract: method_is_abstract,
                        },
                    );
                }
                _ => {
                    self.report_error(
                        "Only method declarations are allowed in trait body".to_string(),
                        stmt.span,
                    );
                }
            }
        }

        context.exit_class();
        context.exit_scope();

        // Create trait definition
        let trait_def = TraitDefinition {
            name: name.clone(),
            generics: generic_defs,
            parent_traits: parent_trait_names,
            methods,
            module: self.current_module.clone(),
        };

        // Register trait type definition
        context.define_type(name.clone(), TypeDefinition::Trait(trait_def.clone()));
        if context.scopes.len() == 1 {
            self.register_type_definition(name.clone(), TypeDefinition::Trait(trait_def));
        }

        // Define trait type symbol
        let trait_type = make_type(TypeKind::Custom(name.clone(), None));

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.clone(),
                SymbolInfo::new(
                    make_type(TypeKind::Meta(Box::new(trait_type.clone()))),
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
                make_type(TypeKind::Meta(Box::new(trait_type))),
                false,
                false,
                visibility.clone(),
                self.current_module.clone(),
                None,
            ),
        );
    }
}
