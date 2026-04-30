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
use crate::ast::types::{TypeDeclarationKind, TypeKind};
use crate::ast::*;
use crate::type_checker::context::{
    AliasDefinition, Context, GenericDefinition, StructDefinition, SymbolInfo, TypeDefinition,
};
use crate::type_checker::TypeChecker;

pub mod control_flow;
pub mod declarations;
pub mod helpers;
pub mod imports;
pub mod returns;
pub mod variables;

pub(crate) use declarations::FunctionDeclarationInfo;
pub(crate) use returns::check_returns;
pub(crate) use returns::ReturnStatus;

impl TypeChecker {
    /// Checks a statement for type correctness.
    ///
    /// This method handles variable declarations, control flow, function declarations,
    /// and other statement types.
    pub(crate) fn check_statement(&mut self, statement: &Statement, context: &mut Context) {
        match &statement.node {
            StatementKind::Variable(decls, vis) => {
                self.check_variable_declaration(decls, vis, context, statement.span)
            }
            StatementKind::Expression(expr) => {
                self.infer_expression(expr, context);
            }
            StatementKind::Block(stmts) => self.check_block(stmts, context),
            StatementKind::If(cond, then_block, else_block, _) => {
                self.check_if(cond, then_block, else_block, context)
            }
            StatementKind::While(cond, body, _) => self.check_while(cond, body, context),
            StatementKind::For(decls, iterable, body) => {
                self.check_for(decls, iterable, body, context)
            }
            StatementKind::Break => self.check_break(context, statement.span),
            StatementKind::Continue => self.check_continue(context, statement.span),
            StatementKind::Return(expr) => self.check_return(expr, context, statement.span),
            StatementKind::FunctionDeclaration(decl) => self.check_function_declaration(
                FunctionDeclarationInfo {
                    name: &decl.name,
                    generics: &decl.generics,
                    params: &decl.params,
                    return_type: &decl.return_type,
                    body: decl.body.as_ref().map(|b| b.as_ref()),
                    properties: &decl.properties,
                },
                context,
            ),
            StatementKind::Struct(name, generics, fields, methods, vis) => {
                self.check_struct(name, generics, fields, methods, vis, context)
            }
            StatementKind::Enum(name, generics, variants, vis) => {
                self.check_enum(name, generics, variants, vis, context)
            }
            StatementKind::Class(class_data) => self.check_class(
                &class_data.name,
                &class_data.generics,
                &class_data.base_class,
                &class_data.traits,
                &class_data.body,
                &class_data.visibility,
                context,
                statement.span,
                class_data.is_abstract,
            ),
            StatementKind::Trait(name, generics, parent_traits, body, vis) => self.check_trait(
                name,
                generics,
                parent_traits,
                body,
                vis,
                context,
                statement.span,
            ),
            StatementKind::Type(exprs, visibility) => {
                self.check_type_statement(exprs, visibility, context)
            }
            StatementKind::RuntimeFunctionDeclaration(_runtime, name, params, return_type_expr) => {
                // Runtime functions are extern bindings. Register their type
                // signature in the current scope so calls can be type-checked,
                // but skip body checking since they have no body.
                let func_type = make_type(TypeKind::Function(Box::new(FunctionTypeData {
                    generics: None,
                    params: params.to_vec(),
                    return_type: return_type_expr.clone(),
                })));

                if context.scopes.len() == 1 {
                    self.global_scope.insert(
                        name.to_string(),
                        SymbolInfo::new(
                            func_type.clone(),
                            false,
                            false,
                            MemberVisibility::Private,
                            self.current_module.clone(),
                            None,
                        ),
                    );
                }

                context.define(
                    name.to_string(),
                    SymbolInfo::new(
                        func_type,
                        false,
                        false,
                        MemberVisibility::Private,
                        self.current_module.clone(),
                        None,
                    ),
                );

                // Resolve parameter types to catch errors early
                for param in params {
                    self.resolve_type_expression(&param.typ, context);
                }

                // Resolve return type if present
                if let Some(rt_expr) = return_type_expr {
                    self.resolve_type_expression(rt_expr, context);
                }
            }
            StatementKind::Use(path_expr, alias) => {
                self.check_use(path_expr, alias, context);
            }
            // These statement kinds require no type checking:
            // - Empty: no-op
            // - Break/Continue: validated above via check_break/check_continue match arms
            StatementKind::Empty => {}
        }
    }

    pub(crate) fn check_type_statement(
        &mut self,
        exprs: &[Expression],
        _visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        for expr in exprs {
            if let ExpressionKind::TypeDeclaration(name_expr, generics, kind, target_expr) =
                &expr.node
            {
                if let Ok(name) = self.extract_name(name_expr) {
                    // Reject incomplete type declarations (type A, B, C without is/extends/implements)
                    if *kind == TypeDeclarationKind::None && target_expr.is_none() {
                        self.report_error(
                            format!(
                                "Incomplete type declaration '{}'. Use 'is', 'extends', 'implements', or 'includes' to define the type.",
                                name
                            ),
                            expr.span,
                        );
                        continue;
                    }
                    let name = name.to_string();

                    // Handle "type F is map<string, int>" or "type Optional<T> is T?"
                    if *kind == TypeDeclarationKind::Is {
                        if let Some(target) = target_expr {
                            // Extract generic definitions from the generics expression
                            let generic_defs = if let Some(gens) = generics {
                                let mut defs = Vec::with_capacity(gens.len());
                                for gen in gens {
                                    if let ExpressionKind::GenericType(
                                        name_expr,
                                        constraint_expr,
                                        gen_kind,
                                    ) = &gen.node
                                    {
                                        let gen_name = if let ExpressionKind::Identifier(n, _) =
                                            &name_expr.node
                                        {
                                            n.clone()
                                        } else {
                                            continue;
                                        };
                                        let constraint_type = constraint_expr
                                            .as_ref()
                                            .map(|c| self.resolve_type_expression(c, context));
                                        defs.push(GenericDefinition {
                                            name: gen_name,
                                            constraint: constraint_type,
                                            kind: gen_kind.clone(),
                                        });
                                    }
                                }
                                if defs.is_empty() {
                                    None
                                } else {
                                    Some(defs)
                                }
                            } else {
                                None
                            };

                            // If there are generics, define them in a temporary scope before resolving the type
                            if let Some(ref gens) = generics {
                                context.enter_scope();
                                self.define_generics(gens, context);
                            }

                            let target_type = self.resolve_type_expression(target, context);

                            if generics.is_some() {
                                context.exit_scope();
                            }

                            self.register_type_definition(
                                name.clone(),
                                TypeDefinition::Alias(AliasDefinition {
                                    template: target_type,
                                    generics: generic_defs,
                                }),
                            );
                        }
                    } else if let Some(target) = target_expr {
                        // For extends/implements/includes, check for conflicts with existing types
                        if self.is_type_visible(&name) {
                            self.report_error(
                                format!(
                                    "Type '{}' is already defined. Cannot use 'type' statement with '{}' on an existing type.",
                                    name, kind
                                ),
                                expr.span,
                            );
                            continue;
                        }

                        // Validate that target type exists
                        if let Ok(target_name) = self.extract_type_name(target) {
                            if !self.is_type_visible(target_name) {
                                self.report_error(
                                    format!("Unknown type '{}' in type declaration", target_name),
                                    target.span,
                                );
                                continue;
                            }
                            let target_name = target_name.to_string();

                            // Register new type and add hierarchy relationship
                            self.register_type_definition(
                                name.clone(),
                                TypeDefinition::Struct(StructDefinition {
                                    fields: vec![],
                                    generics: None,
                                    has_drop: false,
                                    module: self.current_module.clone(),
                                }),
                            );

                            let entry = self.hierarchy.entry(name.clone()).or_default();
                            match kind {
                                TypeDeclarationKind::Extends => entry.extends = Some(target_name),
                                TypeDeclarationKind::Implements => {
                                    entry.implements.push(target_name)
                                }
                                TypeDeclarationKind::Includes => entry.includes.push(target_name),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
}
