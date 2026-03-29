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
use crate::type_checker::context::{Context, SymbolInfo};
use crate::type_checker::statements::{check_returns, ReturnStatus};
use crate::type_checker::TypeChecker;

pub(crate) struct FunctionDeclarationInfo<'a> {
    pub name: &'a str,
    pub generics: &'a Option<Vec<Expression>>,
    pub params: &'a [Parameter],
    pub return_type: &'a Option<Box<Expression>>,
    pub body: Option<&'a Statement>, // None for abstract functions
    pub properties: &'a FunctionProperties,
}

impl TypeChecker {
    /// Type-checks a function declaration.
    ///
    /// Registers the function in the appropriate scope, validates parameter types,
    /// guards, return type, and checks the function body for type correctness.
    /// Handles GPU functions, async functions, and implicit return type inference.
    pub(crate) fn check_function_declaration(
        &mut self,
        info: FunctionDeclarationInfo,
        context: &mut Context,
    ) {
        let FunctionDeclarationInfo {
            name,
            generics,
            params,
            return_type: return_type_expr,
            body,
            properties,
        } = info;

        let func_type = make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: generics.clone(),
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
                    properties.visibility.clone(),
                    self.current_module.clone(),
                    None,
                ),
            );
        }

        // Don't register class methods as bare functions — they must be
        // called via `self.method()`, not `method()`.
        if !context.in_class() {
            context.define(
                name.to_string(),
                SymbolInfo::new(
                    func_type,
                    false,
                    false,
                    properties.visibility.clone(),
                    self.current_module.clone(),
                    None,
                ),
            );
        }

        context.enter_scope();

        if let Some(gens) = generics {
            self.define_generics(gens, context);
        }

        let return_type = if let Some(rt_expr) = return_type_expr {
            self.resolve_type_expression(rt_expr, context)
        } else {
            make_type(TypeKind::Void)
        };

        context.return_types.push(return_type.clone());
        context.inferred_return_types.push(None);

        // Reset loop depth for function body as it's a new context
        let old_loop_depth = context.loop_depth;
        context.loop_depth = 0;

        // If this is 'main' with implicit return type, we might infer it from the body
        let infer_main_return = name == "main" && return_type_expr.is_none();

        for param in params {
            let param_type = self.resolve_type_expression(&param.typ, context);

            if let Some(default_val) = &param.default_value {
                let default_val_type = self.infer_expression(default_val, context);
                if !self.are_compatible(&param_type, &default_val_type, context) {
                    self.report_error(
                        format!(
                            "Type mismatch for default value: expected {}, got {}",
                            param_type, default_val_type
                        ),
                        default_val.span,
                    );
                }
            }

            context.define(
                param.name.clone(),
                SymbolInfo::new(
                    param_type,
                    false,
                    false,
                    MemberVisibility::Public,
                    self.current_module.clone(),
                    None,
                ),
            );
            // Parameters are immutable by default

            if let Some(guard) = &param.guard {
                if let ExpressionKind::Guard(op, right) = &guard.node {
                    let bin_op = match op {
                        GuardOp::NotEqual => BinaryOp::NotEqual,
                        GuardOp::LessThan => BinaryOp::LessThan,
                        GuardOp::LessThanEqual => BinaryOp::LessThanEqual,
                        GuardOp::GreaterThan => BinaryOp::GreaterThan,
                        GuardOp::GreaterThanEqual => BinaryOp::GreaterThanEqual,
                        GuardOp::In => BinaryOp::In,
                        GuardOp::NotIn => BinaryOp::In, // Type check is same as In
                        GuardOp::Not => BinaryOp::NotEqual, // Assumption: not is !=
                    };

                    let left =
                        crate::ast::factory::identifier_with_span(&param.name, param.typ.span);
                    let guard_type = self.infer_binary(&left, &bin_op, right, guard.span, context);

                    if !matches!(guard_type.kind, TypeKind::Boolean) {
                        self.report_error(
                            format!("Type mismatch: guard must be boolean, got {}", guard_type),
                            guard.span,
                        );
                    }
                }
            }
        }

        // Handle GPU functions
        let previous_in_gpu = context.in_gpu_function;
        if properties.is_gpu {
            context.in_gpu_function = true;

            // Enforce NO explicit return type in source code
            if let Some(rt_expr) = return_type_expr {
                self.report_error(
                    "GPU functions must not have an explicit return type".to_string(),
                    rt_expr.span,
                );
            }

            // Implicitly set return type to Kernel
            // Note: The `func_type` symbol stored in global_scope above was created using `return_type_expr`.
            // We need to update that symbol to return `Kernel` so that calls to it are typed correctly.
            let kernel_return_type = make_type(TypeKind::Custom("Kernel".to_string(), None));

            if let Some(info) = self.global_scope.get_mut(name) {
                if let TypeKind::Function(func) = &info.ty.kind {
                    info.ty = make_type(TypeKind::Function(Box::new(FunctionTypeData {
                        generics: func.generics.clone(),
                        params: func.params.clone(),
                        return_type: Some(Box::new(crate::ast::factory::type_expr_non_null(
                            kernel_return_type.clone(),
                        ))),
                    })));
                }
            }
            context.update_symbol_type(
                name,
                make_type(TypeKind::Function(Box::new(FunctionTypeData {
                    generics: generics.clone(),
                    params: params.to_vec(),
                    return_type: Some(Box::new(crate::ast::factory::type_expr_non_null(
                        kernel_return_type.clone(),
                    ))),
                }))),
            );

            // Inject 'gpu_context' object (type GpuContext)
            let gpu_context_type = make_type(TypeKind::Custom("GpuContext".to_string(), None));
            context.define(
                "gpu_context".to_string(),
                SymbolInfo::new(
                    gpu_context_type,
                    false, // Immutable
                    false,
                    MemberVisibility::Public,
                    self.current_module.clone(),
                    None,
                ),
            );
        }

        // Track function context for await validation
        let previous_in_function = context.in_function;
        let previous_in_async = context.in_async_function;
        context.in_function = true;
        context.in_async_function = properties.is_async;

        let mut const_value: Option<Literal> = None;

        // Only check function body if it exists (abstract functions have no body)
        if let Some(body) = body {
            match &body.node {
                StatementKind::Block(stmts) => {
                    // Note: Do not enter a new scope here - the function body shares the scope with parameters.

                    // First, check all statements normally
                    for stmt in stmts.iter() {
                        self.check_statement(stmt, context);
                    }

                    // For implicit return inference, find the last meaningful statement
                    // (skip trailing empty blocks which can be created by trailing whitespace)
                    if infer_main_return {
                        // Find the last non-empty statement that could provide a return value
                        let last_meaningful_stmt = stmts.iter().rev().find(|stmt| {
                            !matches!(&stmt.node, StatementKind::Block(inner) if inner.is_empty())
                        });

                        if let Some(stmt) = last_meaningful_stmt {
                            if let Some(expr_type) = self.resolve_implicit_return_type(stmt) {
                                self.register_implicit_main_return(name, expr_type, context);
                            }
                        }
                    } else if !matches!(return_type.kind, TypeKind::Void) {
                        // For non-main functions with explicit return type, check the last expression
                        if let Some(last_stmt) = stmts.last() {
                            if let StatementKind::Expression(expr) = &last_stmt.node {
                                let expr_type = self.infer_expression(expr, context);
                                if !self.are_compatible(&return_type, &expr_type, context) {
                                    self.report_error(
                                        format!(
                                            "Invalid return type: expected {}, got {}",
                                            return_type, expr_type
                                        ),
                                        expr.span,
                                    );
                                }
                            }
                        }
                    }
                }
                StatementKind::Expression(expr) => {
                    let expr_type = self.infer_expression(expr, context);

                    if !infer_main_return
                        && !matches!(return_type.kind, TypeKind::Void)
                        && !self.are_compatible(&return_type, &expr_type, context)
                    {
                        self.report_error(
                            format!(
                                "Invalid return type: expected {}, got {}",
                                return_type, expr_type
                            ),
                            expr.span,
                        );
                    }

                    if infer_main_return {
                        // Implicit return for single-expression main
                        self.register_implicit_main_return(name, expr_type, context);
                    }
                }
                _ => {
                    self.check_statement(body, context);
                }
            }

            if let StatementKind::Expression(expr) = &body.node {
                if let ExpressionKind::Literal(lit) = &expr.node {
                    const_value = Some(lit.clone());
                }
            } else if let StatementKind::Block(stmts) = &body.node {
                if stmts.len() == 1 {
                    if let StatementKind::Expression(expr) = &stmts[0].node {
                        if let ExpressionKind::Literal(lit) = &expr.node {
                            const_value = Some(lit.clone());
                        }
                    }
                }
            }

            if !matches!(return_type.kind, TypeKind::Void) {
                let status = check_returns(body);
                if status == ReturnStatus::None {
                    self.report_error("Missing return statement".to_string(), body.span);
                }
            }
        }

        // If a constant value was found, update the symbol information
        if const_value.is_some() {
            if let Some(info) = self.global_scope.get_mut(name) {
                info.value = const_value.clone();
                info.is_constant = true;
            }
            if let Some(info) = context
                .scopes
                .first_mut()
                .and_then(|scope| scope.get_mut(name))
            {
                info.value = const_value;
                info.is_constant = true;
            }
        }

        context.in_gpu_function = previous_in_gpu;
        context.in_function = previous_in_function;
        context.in_async_function = previous_in_async;
        context.loop_depth = old_loop_depth;
        context.exit_scope();
        context.return_types.pop();
        context.inferred_return_types.pop();
    }
}
