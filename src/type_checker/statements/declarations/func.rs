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
use crate::ast::types::{
    TypeKind, GPU_CONTEXT_DEPRECATED_IDENT, GPU_CONTEXT_TYPE_NAME, KERNEL_CONTEXT_IDENT,
    KERNEL_TYPE_NAME,
};
use crate::ast::*;
use crate::type_checker::context::{Context, SymbolInfo};
use crate::type_checker::statements::{check_returns, ReturnStatus};
use crate::type_checker::utils::is_gpu_compatible;
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

        self.register_function_symbol(
            name,
            generics,
            params,
            return_type_expr,
            properties,
            context,
        );
        context.enter_scope();

        if let Some(gens) = generics {
            self.define_generics(gens, context);
        }

        let return_type = self.resolve_function_return_type(return_type_expr, context);
        context.return_types.push(return_type.clone());
        context.inferred_return_types.push(None);

        let old_loop_depth = context.loop_depth;
        context.loop_depth = 0;
        let old_gpu_for_depth = context.gpu_for_depth;
        context.gpu_for_depth = 0;
        let infer_main_return = name == "main" && return_type_expr.is_none();

        self.check_function_parameters(params, context);

        let previous_in_gpu = context.in_gpu_function;
        self.handle_gpu_function(
            name,
            generics,
            params,
            return_type_expr,
            properties,
            context,
        );

        let previous_in_function = context.in_function;
        let previous_in_async = context.in_async_function;
        context.in_function = true;
        context.in_async_function = properties.is_async;

        // Store the function body for GPU callability analysis
        if let Some(body_stmt) = body {
            self.function_bodies
                .insert(name.to_string(), std::rc::Rc::new(body_stmt.clone()));
        }

        let const_value =
            self.check_function_body(body, name, &return_type, infer_main_return, context);

        if const_value.is_some() {
            self.update_const_symbol(name, const_value, context);
        }

        context.in_gpu_function = previous_in_gpu;
        context.in_function = previous_in_function;
        context.in_async_function = previous_in_async;
        context.loop_depth = old_loop_depth;
        context.gpu_for_depth = old_gpu_for_depth;
        context.exit_scope();
        context.return_types.pop();
        context.inferred_return_types.pop();
    }

    fn register_function_symbol(
        &mut self,
        name: &str,
        generics: &Option<Vec<Expression>>,
        params: &[Parameter],
        return_type_expr: &Option<Box<Expression>>,
        properties: &FunctionProperties,
        context: &mut Context,
    ) {
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
    }

    fn resolve_function_return_type(
        &mut self,
        return_type_expr: &Option<Box<Expression>>,
        context: &mut Context,
    ) -> Type {
        if let Some(rt_expr) = return_type_expr {
            self.resolve_type_expression(rt_expr, context)
        } else {
            make_type(TypeKind::Void)
        }
    }

    fn check_function_parameters(&mut self, params: &[Parameter], context: &mut Context) {
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
                    param_type.clone(),
                    param.is_out,
                    false,
                    MemberVisibility::Public,
                    self.current_module.clone(),
                    None,
                ),
            );

            self.check_parameter_guard(param, &param_type, context);
        }
    }

    fn check_parameter_guard(
        &mut self,
        param: &Parameter,
        _param_type: &Type,
        context: &mut Context,
    ) {
        if let Some(guard) = &param.guard {
            if let ExpressionKind::Guard(op, right) = &guard.node {
                let bin_op = match op {
                    GuardOp::NotEqual => BinaryOp::NotEqual,
                    GuardOp::LessThan => BinaryOp::LessThan,
                    GuardOp::LessThanEqual => BinaryOp::LessThanEqual,
                    GuardOp::GreaterThan => BinaryOp::GreaterThan,
                    GuardOp::GreaterThanEqual => BinaryOp::GreaterThanEqual,
                    GuardOp::In => BinaryOp::In,
                    GuardOp::NotIn => BinaryOp::In,
                    GuardOp::Not => BinaryOp::NotEqual,
                };

                let left = crate::ast::factory::identifier_with_span(&param.name, param.typ.span);
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

    fn handle_gpu_function(
        &mut self,
        name: &str,
        generics: &Option<Vec<Expression>>,
        params: &[Parameter],
        return_type_expr: &Option<Box<Expression>>,
        properties: &FunctionProperties,
        context: &mut Context,
    ) {
        if !properties.is_gpu {
            return;
        }

        context.in_gpu_function = true;

        if let Some(rt_expr) = return_type_expr {
            self.report_error(
                "GPU functions must not have an explicit return type".to_string(),
                rt_expr.span,
            );
        }

        let kernel_return_type = make_type(TypeKind::Custom(KERNEL_TYPE_NAME.to_string(), None));

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

        self.define_kernel_context(context);

        self.check_gpu_function_param_types(params, context);
    }

    /// Binds the implicit kernel context inside a `gpu fn` body.
    ///
    /// `kernel` is the canonical identifier; `gpu_context` is the deprecated
    /// alias kept for one release. Both resolve to the same `GpuContext` type;
    /// a use of the alias is flagged when its type is inferred.
    fn define_kernel_context(&mut self, context: &mut Context) {
        let context_type = || make_type(TypeKind::Custom(GPU_CONTEXT_TYPE_NAME.to_string(), None));
        let symbol = |ty| {
            SymbolInfo::new(
                ty,
                false,
                false,
                MemberVisibility::Public,
                self.current_module.clone(),
                None,
            )
        };
        context.define(KERNEL_CONTEXT_IDENT.to_string(), symbol(context_type()));
        context.define(
            GPU_CONTEXT_DEPRECATED_IDENT.to_string(),
            symbol(context_type()),
        );
    }

    /// Rejects `gpu fn` parameters whose declared type is not GPU-compatible.
    ///
    /// Runs after `check_function_parameters` has resolved each `param.typ` and
    /// defined the parameter symbol, so the resolved type is already authoritative.
    /// Mirrors [`Self::check_gpu_variable_type`] for the parameter list.
    fn check_gpu_function_param_types(&mut self, params: &[Parameter], context: &mut Context) {
        for param in params {
            let param_type = self.resolve_type_expression(&param.typ, context);
            if matches!(param_type.kind, TypeKind::Error) {
                continue;
            }
            if is_gpu_compatible(&param_type.kind) {
                continue;
            }
            self.report_error(
                format!(
                    "Parameter '{}' has type '{}' which is not GPU-compatible: only numeric primitives, booleans, and GPU types may appear in a 'gpu fn' signature",
                    param.name, param_type
                ),
                param.typ.span,
            );
        }
    }

    fn check_function_body(
        &mut self,
        body: Option<&Statement>,
        name: &str,
        return_type: &Type,
        infer_main_return: bool,
        context: &mut Context,
    ) -> Option<Literal> {
        let mut const_value: Option<Literal> = None;

        if let Some(body) = body {
            const_value = self.extract_const_value(body);
            self.validate_function_body(body, name, return_type, infer_main_return, context);
        }

        const_value
    }

    fn extract_const_value(&self, body: &Statement) -> Option<Literal> {
        if let StatementKind::Expression(expr) = &body.node {
            if let ExpressionKind::Literal(lit) = &expr.node {
                return Some(lit.clone());
            }
        } else if let StatementKind::Block(stmts) = &body.node {
            if stmts.len() == 1 {
                if let StatementKind::Expression(expr) = &stmts[0].node {
                    if let ExpressionKind::Literal(lit) = &expr.node {
                        return Some(lit.clone());
                    }
                }
            }
        }
        None
    }

    fn validate_function_body(
        &mut self,
        body: &Statement,
        name: &str,
        return_type: &Type,
        infer_main_return: bool,
        context: &mut Context,
    ) {
        match &body.node {
            StatementKind::Block(stmts) => {
                self.validate_block_body(stmts, name, return_type, infer_main_return, context);
            }
            StatementKind::Expression(expr) => {
                self.validate_expression_body(expr, name, return_type, infer_main_return, context);
            }
            _ => {
                self.check_statement(body, context);
            }
        }

        self.check_return_completeness(body, return_type);
    }

    fn validate_block_body(
        &mut self,
        stmts: &[Statement],
        name: &str,
        return_type: &Type,
        infer_main_return: bool,
        context: &mut Context,
    ) {
        let last_idx = stmts.len().saturating_sub(1);
        let is_non_void_return = !infer_main_return && !matches!(return_type.kind, TypeKind::Void);
        for (i, stmt) in stmts.iter().enumerate() {
            if i == last_idx
                && is_non_void_return
                && matches!(stmt.node, StatementKind::Expression(_))
            {
                context.suppress_must_use = true;
            }
            self.check_statement(stmt, context);
            context.suppress_must_use = false;
        }

        if infer_main_return {
            let last_meaningful_stmt = stmts.iter().rev().find(
                |stmt| !matches!(&stmt.node, StatementKind::Block(inner) if inner.is_empty()),
            );

            if let Some(stmt) = last_meaningful_stmt {
                if let Some(expr_type) = self.resolve_implicit_return_type(stmt) {
                    self.register_implicit_main_return(name, expr_type, context);
                }
            }
        } else if !matches!(return_type.kind, TypeKind::Void) {
            if let Some(last_stmt) = stmts.last() {
                if let StatementKind::Expression(expr) = &last_stmt.node {
                    let expr_type = self.infer_expression(expr, context);
                    if !self.are_compatible(return_type, &expr_type, context) {
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

    fn validate_expression_body(
        &mut self,
        expr: &Expression,
        name: &str,
        return_type: &Type,
        infer_main_return: bool,
        context: &mut Context,
    ) {
        let expr_type = self.infer_expression(expr, context);

        if !infer_main_return
            && !matches!(return_type.kind, TypeKind::Void)
            && !self.are_compatible(return_type, &expr_type, context)
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
            self.register_implicit_main_return(name, expr_type, context);
        }
    }

    fn check_return_completeness(&mut self, body: &Statement, return_type: &Type) {
        if !matches!(return_type.kind, TypeKind::Void) {
            let status = check_returns(body);
            if status == ReturnStatus::None {
                self.report_error("Missing return statement".to_string(), body.span);
            }
        }
    }

    fn update_const_symbol(
        &mut self,
        name: &str,
        const_value: Option<Literal>,
        context: &mut Context,
    ) {
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
    }
}
