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
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, SymbolInfo};
use crate::type_checker::statements::returns::collect_tail_expression_spans;
use crate::type_checker::statements::{check_returns, ReturnStatus};
use crate::type_checker::utils::is_gpu_signature_type;
use crate::type_checker::TypeChecker;

pub(crate) struct FunctionDeclarationInfo<'a> {
    pub name: &'a str,
    pub generics: &'a Option<Vec<Expression>>,
    pub params: &'a [Parameter],
    pub return_type: &'a Option<Box<Expression>>,
    pub body: Option<&'a Statement>, // None for abstract functions
    pub properties: &'a FunctionProperties,
    pub span: Span,
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
            span,
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

        // Validate parameter residency annotations
        self.validate_parameter_residencies(params, properties);

        if properties.is_gpu {
            self.check_gpu_function_name(name, span);
        }

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
        let previous_in_static_method = context.in_static_method;
        context.in_function = true;
        context.in_async_function = properties.is_async;
        context.in_static_method = properties.is_static;

        // Store the function body for GPU callability analysis
        if let Some(body_stmt) = body {
            self.fn_analysis
                .function_bodies
                .insert(name.to_string(), std::rc::Rc::new(body_stmt.clone()));

            // Compute residency verdict for this function
            let residency = self.compute_function_residency(params, body_stmt, context);
            self.fn_analysis
                .fn_residencies
                .insert(name.to_string(), residency);
        }

        // Store the out-param flags for each function (used in GPU kernel launch)
        let out_flags: Vec<bool> = params.iter().map(|p| p.is_out).collect();
        self.fn_analysis
            .function_out_params
            .insert(name.to_string(), out_flags);

        let const_value =
            self.check_function_body(body, name, &return_type, infer_main_return, context);

        if const_value.is_some() {
            self.update_const_symbol(name, const_value, context);
        }

        // Reject barriers under thread-divergent control flow, and non-injective
        // buffer writes, in GPU kernels. Both run while the parameters are still
        // resolvable in the current scope.
        if properties.is_gpu {
            if let Some(body_stmt) = body {
                self.check_barrier_uniformity(body_stmt);
                self.check_kernel_concurrent_writes(params, body_stmt, context);
            }
        }

        context.in_gpu_function = previous_in_gpu;
        context.in_function = previous_in_function;
        context.in_async_function = previous_in_async;
        context.in_static_method = previous_in_static_method;
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
            let mut global_info = SymbolInfo::new(
                func_type.clone(),
                false,
                false,
                properties.visibility.clone(),
                self.modules.current_module.clone(),
                None,
            );
            global_info.is_gpu_fn = properties.is_gpu;
            self.type_table
                .global_scope
                .insert(name.to_string(), global_info);
        }

        if !context.in_class() {
            let mut local_info = SymbolInfo::new(
                func_type,
                false,
                false,
                properties.visibility.clone(),
                self.modules.current_module.clone(),
                None,
            );
            local_info.is_gpu_fn = properties.is_gpu;
            context.define(name.to_string(), local_info);
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
                        DiagnosticCode::TypTypeMismatch,
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
                    self.modules.current_module.clone(),
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
                        DiagnosticCode::TypTypeMismatch,
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
                DiagnosticCode::TarGpuIncompatibleSignature,
                "GPU functions must not have an explicit return type".to_string(),
                rt_expr.span,
            );
        }

        let kernel_return_type = make_type(TypeKind::Custom(KERNEL_TYPE_NAME.to_string(), None));

        if let Some(info) = self.type_table.global_scope.get_mut(name) {
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

    /// Rejects a `gpu fn` whose name collides with a WGSL reserved form.
    ///
    /// The name is emitted verbatim as the WGSL entry-point/helper name, so a
    /// reserved name (a `__` prefix or a WGSL keyword) would otherwise fail late
    /// at shader-module compilation with a generic backend error. Reporting here
    /// gives a source-cited diagnostic and a rename hint instead.
    fn check_gpu_function_name(&mut self, name: &str, span: Span) {
        let Some(conflict) = crate::mir::backend::gpu::wgsl_name_conflict(name) else {
            return;
        };
        self.report_error_with_help(DiagnosticCode::TarGpuIncompatibleSignature,
            format!(
                "GPU function name '{}' {}, so it cannot be lowered to a valid GPU shader",
                name,
                conflict.describe()
            ),
            span,
            format!("Rename '{name}' to a name that is not a reserved WGSL keyword and does not begin with '__'."),
        );
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
                self.modules.current_module.clone(),
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
            if is_gpu_signature_type(&param_type.kind) {
                continue;
            }
            self.report_error(DiagnosticCode::TarGpuIncompatibleSignature,
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
                let is_non_void_return =
                    !infer_main_return && !matches!(return_type.kind, TypeKind::Void);
                if is_non_void_return {
                    self.check_statement_with_tail_exemptions(body, context);
                } else {
                    self.check_statement(body, context);
                }
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
            if i == last_idx && is_non_void_return {
                self.check_statement_with_tail_exemptions(stmt, context);
            } else {
                self.check_statement(stmt, context);
            }
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
                DiagnosticCode::TypTypeMismatch,
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

    /// Checks a statement with tail expression spans exempted from must_use checking.
    ///
    /// Collects the tail expression spans from the statement, installs them in the
    /// context, checks the statement, and restores the previous exempt spans.
    fn check_statement_with_tail_exemptions(&mut self, stmt: &Statement, context: &mut Context) {
        let mut tail_spans = Vec::new();
        collect_tail_expression_spans(stmt, &mut tail_spans);
        let prev_spans = std::mem::take(&mut context.must_use_exempt_spans);
        context.must_use_exempt_spans.extend(tail_spans);
        self.check_statement(stmt, context);
        context.must_use_exempt_spans = prev_spans;
    }

    fn check_return_completeness(&mut self, body: &Statement, return_type: &Type) {
        if !matches!(return_type.kind, TypeKind::Void) {
            let status = check_returns(body);
            if status == ReturnStatus::None {
                self.report_error(
                    DiagnosticCode::TypFunctionSignature,
                    "Missing return statement".to_string(),
                    body.span,
                );
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
            if let Some(info) = self.type_table.global_scope.get_mut(name) {
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

    /// Computes the residency verdict for a function: whether it can safely
    /// accept gpu-resident arguments. Returns one of:
    /// - PolymorphicSafe: only buffer-untouching operations (e.g., .length() on fixed arrays)
    /// - GpuLaunchSafe: buffer access only inside forall bodies (device-context)
    /// - HostOnly: host-forcing operations or buffer access in host context
    fn compute_function_residency(
        &self,
        params: &[Parameter],
        body: &Statement,
        context: &Context,
    ) -> crate::type_checker::FnResidency {
        let param_names: std::collections::HashSet<_> =
            params.iter().map(|p| p.name.clone()).collect();

        if self.body_forces_host(body) {
            return crate::type_checker::FnResidency::HostOnly;
        }

        // Check if params are touched and in what context
        let host_touch =
            self.body_touches_param_buffer_in_host_context(body, &param_names, params, context);
        if host_touch {
            return crate::type_checker::FnResidency::HostOnly;
        }

        // Check if params are touched only in device context (inside forall)
        let device_touch =
            self.body_touches_param_buffer_in_device_context(body, &param_names, params, context);
        if device_touch {
            return crate::type_checker::FnResidency::GpuLaunchSafe;
        }

        // No buffer touching at all
        crate::type_checker::FnResidency::PolymorphicSafe
    }

    /// Checks if a function body calls any host-forcing intrinsic (println, etc).
    fn body_forces_host(&self, stmt: &Statement) -> bool {
        match &stmt.node {
            StatementKind::Expression(expr) => self.expr_forces_host(expr),
            StatementKind::Variable(vars, _visibility) => {
                // Check if any initializer contains host-forcing intrinsics
                vars.iter().any(|v| {
                    v.initializer
                        .as_ref()
                        .is_some_and(|e| self.expr_forces_host(e))
                })
            }
            StatementKind::If(cond, then_stmt, else_stmt, _if_type) => {
                self.expr_forces_host(cond)
                    || self.body_forces_host(then_stmt)
                    || else_stmt.as_ref().is_some_and(|s| self.body_forces_host(s))
            }
            StatementKind::While(cond, body_stmt, _while_type) => {
                self.expr_forces_host(cond) || self.body_forces_host(body_stmt)
            }
            StatementKind::For(_vars, iterable, body_stmt) => {
                self.expr_forces_host(iterable) || self.body_forces_host(body_stmt)
            }
            StatementKind::Forall {
                device: _,
                vars: _,
                iterable,
                body: body_stmt,
            } => self.expr_forces_host(iterable) || self.body_forces_host(body_stmt),
            StatementKind::Return(expr_opt) => expr_opt
                .as_ref()
                .is_some_and(|expr| self.expr_forces_host(expr)),
            StatementKind::Block(stmts) => stmts.iter().any(|s| self.body_forces_host(s)),
            // Genuinely inert statement kinds
            StatementKind::Empty | StatementKind::Break | StatementKind::Continue => false,
            StatementKind::Use(_, _) | StatementKind::Type(_, _) => false,
            StatementKind::Enum(_, _, _, _, _, _) => false,
            StatementKind::Struct(_, _, _, _, _, _) => false,
            StatementKind::Class(_) => false,
            StatementKind::Trait(_, _, _, _, _) => false,
            StatementKind::FunctionDeclaration(_) => {
                // Nested function declarations don't execute host code in outer scope
                false
            }
            StatementKind::RuntimeFunctionDeclaration(_, _, _, _) => false,
            StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => false,
            StatementKind::GpuFrame(_, _, _) => false,
            StatementKind::GpuFrameBlock(_) => false,
        }
    }

    /// Checks if an expression calls any host-forcing intrinsic.
    fn expr_forces_host(&self, expr: &Expression) -> bool {
        match &expr.node {
            ExpressionKind::Call(func_expr, args) => {
                if let ExpressionKind::Identifier(name, _) = &func_expr.node {
                    // Check if this is a host intrinsic
                    if matches!(
                        name.as_str(),
                        "println" | "print" | "printf" | "eprintln" | "eprint" | "input" | "readln"
                    ) {
                        return true;
                    }
                }
                args.iter().any(|arg| self.expr_forces_host(arg))
            }
            ExpressionKind::Binary(left, _, right) => {
                self.expr_forces_host(left) || self.expr_forces_host(right)
            }
            ExpressionKind::Unary(_, expr) => self.expr_forces_host(expr),
            ExpressionKind::Member(_, _) | ExpressionKind::Index(_, _) => {
                // Don't recurse; index/member access is buffer-touching, not host-forcing
                false
            }
            _ => false,
        }
    }

    /// Checks if a function body performs buffer-touching operations on params.
    /// Buffer-touching includes element indexing or capture in gpu forall.
    fn body_touches_param_buffer(
        &self,
        stmt: &Statement,
        param_names: &std::collections::HashSet<String>,
        params: &[Parameter],
        context: &Context,
    ) -> bool {
        match &stmt.node {
            StatementKind::Expression(expr) => {
                self.expr_touches_param_buffer(expr, param_names, params, context)
            }
            StatementKind::Variable(vars, _visibility) => {
                // Check if any initializer touches param buffer (e.g., let b = a where a is param)
                vars.iter().any(|v| {
                    v.initializer.as_ref().is_some_and(|e| {
                        self.expr_touches_param_buffer(e, param_names, params, context)
                    })
                })
            }
            StatementKind::If(cond, then_stmt, else_stmt, _if_type) => {
                self.expr_touches_param_buffer(cond, param_names, params, context)
                    || self.body_touches_param_buffer(then_stmt, param_names, params, context)
                    || else_stmt.as_ref().is_some_and(|s| {
                        self.body_touches_param_buffer(s, param_names, params, context)
                    })
            }
            StatementKind::While(cond, body_stmt, _while_type) => {
                self.expr_touches_param_buffer(cond, param_names, params, context)
                    || self.body_touches_param_buffer(body_stmt, param_names, params, context)
            }
            StatementKind::For(_vars, iterable, body_stmt) => {
                self.expr_touches_param_buffer(iterable, param_names, params, context)
                    || self.body_touches_param_buffer(body_stmt, param_names, params, context)
            }
            StatementKind::Forall {
                device: _,
                vars: _,
                iterable,
                body: body_stmt,
            } => {
                self.expr_touches_param_buffer(iterable, param_names, params, context)
                    || self.body_touches_param_buffer(body_stmt, param_names, params, context)
            }
            StatementKind::Return(expr_opt) => expr_opt.as_ref().is_some_and(|expr| {
                self.expr_touches_param_buffer(expr, param_names, params, context)
            }),
            StatementKind::Block(stmts) => stmts
                .iter()
                .any(|s| self.body_touches_param_buffer(s, param_names, params, context)),
            // Genuinely inert statement kinds
            StatementKind::Empty | StatementKind::Break | StatementKind::Continue => false,
            StatementKind::Use(_, _) | StatementKind::Type(_, _) => false,
            StatementKind::Enum(_, _, _, _, _, _) => false,
            StatementKind::Struct(_, _, _, _, _, _) => false,
            StatementKind::Class(_) => false,
            StatementKind::Trait(_, _, _, _, _) => false,
            StatementKind::FunctionDeclaration(_) => {
                // Nested function declarations don't reference params
                false
            }
            StatementKind::RuntimeFunctionDeclaration(_, _, _, _) => false,
            StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => false,
            StatementKind::GpuFrame(_, _, _) => false,
            StatementKind::GpuFrameBlock(_) => false,
        }
    }

    /// Checks if an expression performs buffer-touching on a parameter.
    /// FAIL-CLOSED: Default is to reject (deny) unless explicitly whitelisted.
    /// The ONLY safe param position: call to .length() on a static array param.
    fn expr_touches_param_buffer(
        &self,
        expr: &Expression,
        param_names: &std::collections::HashSet<String>,
        params: &[Parameter],
        context: &Context,
    ) -> bool {
        match &expr.node {
            // SAFE WHITELIST: param.length() with zero arguments on static arrays only
            ExpressionKind::Call(func_expr, args) => {
                if args.is_empty() {
                    if let ExpressionKind::Member(base, member_expr) = &func_expr.node {
                        if let ExpressionKind::Identifier(method_name, _) = &member_expr.node {
                            if Self::is_gpu_metadata_method(method_name) {
                                if let ExpressionKind::Identifier(name, _) = &base.node {
                                    if param_names.contains(name) {
                                        // Safe only if param is a static Array<T,N>, not List<T>
                                        if self.param_is_static_array(name, params, context) {
                                            return false;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                // Any other call: check func and args for param usage
                self.expr_touches_param_buffer(func_expr, param_names, params, context)
                    || args.iter().any(|arg| {
                        self.expr_touches_param_buffer(arg, param_names, params, context)
                    })
            }

            // UNSAFE: parameter as index base
            ExpressionKind::Index(base, index) => {
                self.expr_touches_param_buffer(base, param_names, params, context)
                    || self.expr_touches_param_buffer(index, param_names, params, context)
            }

            // UNSAFE: binary operations containing param
            ExpressionKind::Binary(left, _, right) => {
                self.expr_touches_param_buffer(left, param_names, params, context)
                    || self.expr_touches_param_buffer(right, param_names, params, context)
            }

            // UNSAFE: unary operations
            ExpressionKind::Unary(_, expr) => {
                self.expr_touches_param_buffer(expr, param_names, params, context)
            }

            // UNSAFE: member access (other than .length() above)
            ExpressionKind::Member(expr, _) => {
                self.expr_touches_param_buffer(expr, param_names, params, context)
            }

            // UNSAFE: assignment/aliasing
            ExpressionKind::Assignment(target, _op, value) => {
                // Check if assignment target or value references param
                let target_touches = match &**target {
                    crate::ast::expression::LeftHandSideExpression::Identifier(expr) => {
                        if let ExpressionKind::Identifier(name, _) = &expr.node {
                            param_names.contains(name)
                        } else {
                            false
                        }
                    }
                    crate::ast::expression::LeftHandSideExpression::Member(expr) => {
                        self.expr_touches_param_buffer(expr, param_names, params, context)
                    }
                    crate::ast::expression::LeftHandSideExpression::Index(expr) => {
                        self.expr_touches_param_buffer(expr, param_names, params, context)
                    }
                };
                target_touches
                    || self.expr_touches_param_buffer(value, param_names, params, context)
            }

            // UNSAFE: direct param identifier usage (forwarding, return, aliasing)
            ExpressionKind::Identifier(name, _) => param_names.contains(name),

            // UNSAFE: formatted strings contain param
            ExpressionKind::FormattedString(parts) => parts
                .iter()
                .any(|expr| self.expr_touches_param_buffer(expr, param_names, params, context)),

            // UNSAFE: param in collections
            ExpressionKind::List(elements) | ExpressionKind::Array(elements, _) => elements
                .iter()
                .any(|e| self.expr_touches_param_buffer(e, param_names, params, context)),
            ExpressionKind::Map(pairs) => pairs.iter().any(|(k, v)| {
                self.expr_touches_param_buffer(k, param_names, params, context)
                    || self.expr_touches_param_buffer(v, param_names, params, context)
            }),
            ExpressionKind::Tuple(elements) | ExpressionKind::Set(elements) => elements
                .iter()
                .any(|e| self.expr_touches_param_buffer(e, param_names, params, context)),

            // UNSAFE: lambda with param access
            ExpressionKind::Lambda(boxed) => {
                self.body_contains_param_identifier(&boxed.body, param_names, params, context)
            }

            // UNSAFE: logical operations with params
            ExpressionKind::Logical(left, _, right) => {
                self.expr_touches_param_buffer(left, param_names, params, context)
                    || self.expr_touches_param_buffer(right, param_names, params, context)
            }

            // UNSAFE: ranges with params
            ExpressionKind::Range(start, end, _range_type) => {
                self.expr_touches_param_buffer(start, param_names, params, context)
                    || end.as_ref().is_some_and(|e| {
                        self.expr_touches_param_buffer(e, param_names, params, context)
                    })
            }

            // UNSAFE: import paths can reference params
            ExpressionKind::ImportPath(parts, _) => parts
                .iter()
                .any(|part| self.expr_touches_param_buffer(part, param_names, params, context)),

            // UNSAFE: block expressions
            ExpressionKind::Block(stmts, expr) => {
                stmts
                    .iter()
                    .any(|s| self.body_contains_param_identifier(s, param_names, params, context))
                    || self.expr_touches_param_buffer(expr, param_names, params, context)
            }

            // SAFE: literals, type refs, super don't reference params
            ExpressionKind::Literal(_)
            | ExpressionKind::Type(_, _)
            | ExpressionKind::GenericType(_, _, _)
            | ExpressionKind::TypeDeclaration(_, _, _, _)
            | ExpressionKind::EnumValue(_, _)
            | ExpressionKind::StructMember(_, _)
            | ExpressionKind::Guard(_, _)
            | ExpressionKind::Cast(_, _)
            | ExpressionKind::NamedArgument(_, _)
            | ExpressionKind::Match(_, _)
            | ExpressionKind::Conditional(_, _, _, _)
            | ExpressionKind::Super => false,
        }
    }

    /// Helper: checks if a body contains direct usage of a param identifier.
    fn body_contains_param_identifier(
        &self,
        stmt: &Statement,
        param_names: &std::collections::HashSet<String>,
        params: &[Parameter],
        context: &Context,
    ) -> bool {
        match &stmt.node {
            StatementKind::Expression(expr) => {
                self.expr_touches_param_buffer(expr, param_names, params, context)
            }
            StatementKind::Variable(_vars, _visibility) => false,
            StatementKind::If(cond, then_stmt, else_stmt, _if_type) => {
                self.expr_touches_param_buffer(cond, param_names, params, context)
                    || self.body_contains_param_identifier(then_stmt, param_names, params, context)
                    || else_stmt.as_ref().is_some_and(|s| {
                        self.body_contains_param_identifier(s, param_names, params, context)
                    })
            }
            StatementKind::While(cond, body_stmt, _while_type) => {
                self.expr_touches_param_buffer(cond, param_names, params, context)
                    || self.body_contains_param_identifier(body_stmt, param_names, params, context)
            }
            StatementKind::For(_vars, iterable, body_stmt) => {
                self.expr_touches_param_buffer(iterable, param_names, params, context)
                    || self.body_contains_param_identifier(body_stmt, param_names, params, context)
            }
            StatementKind::Forall {
                device: _,
                vars: _,
                iterable,
                body: body_stmt,
            } => {
                self.expr_touches_param_buffer(iterable, param_names, params, context)
                    || self.body_contains_param_identifier(body_stmt, param_names, params, context)
            }
            StatementKind::Return(expr_opt) => expr_opt.as_ref().is_some_and(|expr| {
                self.expr_touches_param_buffer(expr, param_names, params, context)
            }),
            StatementKind::Block(stmts) => stmts
                .iter()
                .any(|s| self.body_contains_param_identifier(s, param_names, params, context)),
            _ => false,
        }
    }

    /// Checks if a parameter's declared type is a static array `Array<T,N>`.
    /// Recognizes both forms: `[T;N]` sugar and `Array<T,N>` constructor.
    /// Returns false if the type is dynamic (List<T>) or cannot be determined.
    fn param_is_static_array(
        &self,
        param_name: &str,
        params: &[Parameter],
        _context: &Context,
    ) -> bool {
        let Some(param) = params.iter().find(|p| p.name == param_name) else {
            return false;
        };
        match &param.typ.node {
            ExpressionKind::Type(type_wrapped, _is_nullable) => {
                match &type_wrapped.kind {
                    // [T;N] sugar form
                    TypeKind::Array(_, _) => true,
                    // Array<T,N> constructor form
                    TypeKind::Custom(name, Some(_)) => {
                        use crate::ast::types::BuiltinCollectionKind;
                        BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Array)
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    /// Checks if a function body performs buffer-touching on params in HOST context.
    /// Returns true if params are accessed outside of forall bodies.
    fn body_touches_param_buffer_in_host_context(
        &self,
        stmt: &Statement,
        param_names: &std::collections::HashSet<String>,
        params: &[Parameter],
        context: &Context,
    ) -> bool {
        match &stmt.node {
            StatementKind::Expression(expr) => {
                self.expr_touches_param_buffer_in_host_context(expr, param_names, params, context)
            }
            StatementKind::Variable(vars, _visibility) => vars.iter().any(|v| {
                v.initializer.as_ref().is_some_and(|e| {
                    self.expr_touches_param_buffer_in_host_context(e, param_names, params, context)
                })
            }),
            StatementKind::If(cond, then_stmt, else_stmt, _if_type) => {
                self.expr_touches_param_buffer_in_host_context(cond, param_names, params, context)
                    || self.body_touches_param_buffer_in_host_context(
                        then_stmt,
                        param_names,
                        params,
                        context,
                    )
                    || else_stmt.as_ref().is_some_and(|s| {
                        self.body_touches_param_buffer_in_host_context(
                            s,
                            param_names,
                            params,
                            context,
                        )
                    })
            }
            StatementKind::While(cond, body_stmt, _while_type) => {
                self.expr_touches_param_buffer_in_host_context(cond, param_names, params, context)
                    || self.body_touches_param_buffer_in_host_context(
                        body_stmt,
                        param_names,
                        params,
                        context,
                    )
            }
            StatementKind::For(_vars, iterable, body_stmt) => {
                self.expr_touches_param_buffer_in_host_context(
                    iterable,
                    param_names,
                    params,
                    context,
                ) || self.body_touches_param_buffer_in_host_context(
                    body_stmt,
                    param_names,
                    params,
                    context,
                )
            }
            StatementKind::Forall {
                device: _,
                vars: _,
                iterable,
                body: _body_stmt,
            } => {
                // Forall body is device context: don't count touches inside it as host context
                // Only check the iterable expression (which is evaluated at host)
                self.expr_touches_param_buffer_in_host_context(
                    iterable,
                    param_names,
                    params,
                    context,
                )
            }
            StatementKind::Return(expr_opt) => expr_opt.as_ref().is_some_and(|expr| {
                self.expr_touches_param_buffer_in_host_context(expr, param_names, params, context)
            }),
            StatementKind::Block(stmts) => stmts.iter().any(|s| {
                self.body_touches_param_buffer_in_host_context(s, param_names, params, context)
            }),
            // Inert statement kinds
            StatementKind::Empty | StatementKind::Break | StatementKind::Continue => false,
            StatementKind::Use(_, _) | StatementKind::Type(_, _) => false,
            StatementKind::Enum(_, _, _, _, _, _) => false,
            StatementKind::Struct(_, _, _, _, _, _) => false,
            StatementKind::Class(_) => false,
            StatementKind::Trait(_, _, _, _, _) => false,
            StatementKind::FunctionDeclaration(_) => false,
            StatementKind::RuntimeFunctionDeclaration(_, _, _, _) => false,
            StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => false,
            StatementKind::GpuFrame(_, _, _) => false,
            StatementKind::GpuFrameBlock(_) => false,
        }
    }

    /// Checks if an expression performs buffer-touching on params in HOST context.
    fn expr_touches_param_buffer_in_host_context(
        &self,
        expr: &Expression,
        param_names: &std::collections::HashSet<String>,
        params: &[Parameter],
        context: &Context,
    ) -> bool {
        // Same checks as the original expr_touches_param_buffer
        self.expr_touches_param_buffer(expr, param_names, params, context)
    }

    /// Checks if a function body performs buffer-touching on params in DEVICE context.
    /// Returns true if params are accessed ONLY inside forall bodies.
    fn body_touches_param_buffer_in_device_context(
        &self,
        stmt: &Statement,
        param_names: &std::collections::HashSet<String>,
        params: &[Parameter],
        context: &Context,
    ) -> bool {
        match &stmt.node {
            StatementKind::Expression(_expr) => {
                // Expression outside forall is not device context
                false
            }
            StatementKind::Variable(_vars, _visibility) => {
                // Variable declaration outside forall is not device context
                false
            }
            StatementKind::If(_cond, then_stmt, else_stmt, _if_type) => {
                self.body_touches_param_buffer_in_device_context(
                    then_stmt,
                    param_names,
                    params,
                    context,
                ) || else_stmt.as_ref().is_some_and(|s| {
                    self.body_touches_param_buffer_in_device_context(
                        s,
                        param_names,
                        params,
                        context,
                    )
                })
            }
            StatementKind::While(_cond, body_stmt, _while_type) => self
                .body_touches_param_buffer_in_device_context(
                    body_stmt,
                    param_names,
                    params,
                    context,
                ),
            StatementKind::For(_vars, _iterable, body_stmt) => {
                // For loops are not device context (CPU)
                self.body_touches_param_buffer_in_device_context(
                    body_stmt,
                    param_names,
                    params,
                    context,
                )
            }
            StatementKind::Forall {
                device: _,
                vars: _,
                iterable: _,
                body: body_stmt,
            } => {
                // Inside forall body: any buffer touching here is device context
                self.body_touches_param_buffer(body_stmt, param_names, params, context)
            }
            StatementKind::Return(_expr_opt) => {
                // Return outside forall is not device context
                false
            }
            StatementKind::Block(stmts) => stmts.iter().any(|s| {
                self.body_touches_param_buffer_in_device_context(s, param_names, params, context)
            }),
            // Inert statement kinds
            StatementKind::Empty | StatementKind::Break | StatementKind::Continue => false,
            StatementKind::Use(_, _) | StatementKind::Type(_, _) => false,
            StatementKind::Enum(_, _, _, _, _, _) => false,
            StatementKind::Struct(_, _, _, _, _, _) => false,
            StatementKind::Class(_) => false,
            StatementKind::Trait(_, _, _, _, _) => false,
            StatementKind::FunctionDeclaration(_) => false,
            StatementKind::RuntimeFunctionDeclaration(_, _, _, _) => false,
            StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, _) => false,
            StatementKind::GpuFrame(_, _, _) => false,
            StatementKind::GpuFrameBlock(_) => false,
        }
    }

    /// Validates parameter residency annotations.
    /// A `gpu fn` with an explicit `host` parameter annotation is a conflict.
    fn validate_parameter_residencies(
        &mut self,
        params: &[Parameter],
        properties: &FunctionProperties,
    ) {
        if properties.is_gpu {
            for param in params {
                if let Some(crate::ast::statement::BindingResidency::Host) = param.residency {
                    self.report_error(DiagnosticCode::TypFunctionSignature,
                        format!(
                            "conflict: parameter '{}' cannot be explicitly marked 'host' in a gpu function",
                            param.name
                        ),
                        param.typ.span,
                    );
                }
            }
        }
    }
}
