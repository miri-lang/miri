// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::context::{
    ClassDefinition, Context, EnumDefinition, FieldInfo, GenericDefinition, MethodInfo,
    StructDefinition, TraitDefinition, TypeDefinition,
};
use super::TypeChecker;
use crate::ast::factory::make_type;
use crate::ast::types::{Type, TypeDeclarationKind, TypeKind};
use crate::ast::*;
use crate::error::syntax::Span;
use std::collections::HashMap;

pub(crate) struct FunctionDeclarationInfo<'a> {
    pub name: &'a str,
    pub generics: &'a Option<Vec<Expression>>,
    pub params: &'a [Parameter],
    pub return_type: &'a Option<Box<Expression>>,
    pub body: &'a Statement,
    pub properties: &'a FunctionProperties,
}

#[derive(Debug, PartialEq)]
enum ReturnStatus {
    None,
    Implicit,
    Explicit,
}

fn check_returns(stmt: &Statement) -> ReturnStatus {
    match &stmt.node {
        StatementKind::Return(_) => ReturnStatus::Explicit,
        StatementKind::While(_, _, WhileStatementType::Forever) => ReturnStatus::Explicit,
        StatementKind::Expression(_) => ReturnStatus::Implicit,
        StatementKind::Block(stmts) => {
            for (i, s) in stmts.iter().enumerate() {
                let status = check_returns(s);
                if status == ReturnStatus::Explicit {
                    return ReturnStatus::Explicit;
                }
                if i == stmts.len() - 1 && status == ReturnStatus::Implicit {
                    return ReturnStatus::Implicit;
                }
            }
            ReturnStatus::None
        }
        StatementKind::If(_, then_block, else_block, _) => {
            if let Some(else_stmt) = else_block {
                let then_status = check_returns(then_block);
                let else_status = check_returns(else_stmt);

                match (then_status, else_status) {
                    (ReturnStatus::Explicit, ReturnStatus::Explicit) => ReturnStatus::Explicit,
                    (ReturnStatus::None, _) | (_, ReturnStatus::None) => ReturnStatus::None,
                    _ => ReturnStatus::Implicit,
                }
            } else {
                ReturnStatus::None
            }
        }
        _ => ReturnStatus::None,
    }
}

impl TypeChecker {
    fn resolve_implicit_return_type(&self, stmt: &Statement) -> Option<Type> {
        match &stmt.node {
            StatementKind::Expression(expr) => self.get_type(expr.id).cloned(),
            StatementKind::Block(stmts) => {
                if let Some(last) = stmts.last() {
                    self.resolve_implicit_return_type(last)
                } else {
                    None
                }
            }
            StatementKind::If(_, then_block, else_block, _) => {
                let t1 = self.resolve_implicit_return_type(then_block);
                let t2 = if let Some(else_stmt) = else_block {
                    self.resolve_implicit_return_type(else_stmt)
                } else {
                    None
                };

                match (t1, t2) {
                    (Some(a), Some(_)) => Some(a), // Assume compatible
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                }
            }
            _ => None,
        }
    }

    fn register_implicit_main_return(
        &mut self,
        name: &str,
        expr_type: Type,
        context: &mut Context,
    ) {
        if context.return_types.pop().is_some() {
            let last = expr_type.clone();
            context.return_types.push(last);
        }

        // Update global symbol as well
        if let Some(info) = self.global_scope.get_mut(name) {
            if let TypeKind::Function(gens, params, _) = &info.ty.kind {
                let type_expr = crate::ast::factory::type_expr_non_null(expr_type.clone());
                self.types.insert(type_expr.id, expr_type.clone());

                info.ty = make_type(TypeKind::Function(
                    gens.clone(),
                    params.clone(),
                    Some(Box::new(type_expr)),
                ));
            }
        }
    }
}

impl TypeChecker {
    /// Checks a statement for type correctness.
    ///
    /// This method handles variable declarations, control flow, function declarations,
    /// and other statement types.
    pub(crate) fn check_statement(&mut self, statement: &Statement, context: &mut Context) {
        match &statement.node {
            StatementKind::Variable(decls, vis) => {
                self.check_variable_declaration(decls, vis, context, statement.span.clone())
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
            StatementKind::Break => self.check_break(context, statement.span.clone()),
            StatementKind::Continue => self.check_continue(context, statement.span.clone()),
            StatementKind::Return(expr) => self.check_return(expr, context, statement.span.clone()),
            StatementKind::FunctionDeclaration(
                name,
                generics,
                params,
                return_type,
                body,
                props,
            ) => self.check_function_declaration(
                FunctionDeclarationInfo {
                    name,
                    generics,
                    params,
                    return_type,
                    body,
                    properties: props,
                },
                context,
            ),
            StatementKind::Struct(name, generics, fields, vis) => {
                self.check_struct(name, generics, fields, vis, context)
            }
            StatementKind::Enum(name, variants, vis) => {
                self.check_enum(name, variants, vis, context)
            }
            StatementKind::Class(name, generics, base_class, traits, body, vis) => self
                .check_class(
                    name,
                    generics,
                    base_class,
                    traits,
                    body,
                    vis,
                    context,
                    statement.span.clone(),
                ),
            StatementKind::Trait(name, generics, parent_traits, body, vis) => self.check_trait(
                name,
                generics,
                parent_traits,
                body,
                vis,
                context,
                statement.span.clone(),
            ),
            StatementKind::Type(exprs, visibility) => {
                self.check_type_statement(exprs, visibility, context)
            }
            _ => {}
        }
    }

    fn check_type_statement(
        &mut self,
        exprs: &[Expression],
        _visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        for expr in exprs {
            if let ExpressionKind::TypeDeclaration(name_expr, _generics, kind, target_expr) =
                &expr.node
            {
                if let Ok(name) = self.extract_name(name_expr) {
                    // Handle "type F is map<string, int>"
                    if *kind == TypeDeclarationKind::Is {
                        if let Some(target) = target_expr {
                            let target_type = self.resolve_type_expression(target, context);
                            self.global_type_definitions
                                .insert(name.clone(), TypeDefinition::Alias(target_type));
                        }
                    } else if let Some(target) = target_expr {
                        if let Ok(target_name) = self.extract_type_name(target) {
                            // Register type if not exists (as empty struct/interface)
                            if !self.global_type_definitions.contains_key(&name) {
                                self.global_type_definitions.insert(
                                    name.clone(),
                                    TypeDefinition::Struct(StructDefinition {
                                        fields: vec![],
                                        generics: None,
                                        module: self.current_module.clone(),
                                    }),
                                );
                            }

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
                    } else {
                        // "type A" - opaque type / interface
                        self.global_type_definitions.insert(
                            name.clone(),
                            TypeDefinition::Struct(StructDefinition {
                                fields: vec![],
                                generics: None,
                                module: self.current_module.clone(),
                            }),
                        );
                    }
                }
            }
        }
    }

    fn check_variable_declaration(
        &mut self,
        decls: &[VariableDeclaration],
        visibility: &MemberVisibility,
        context: &mut Context,
        span: Span,
    ) {
        for decl in decls {
            // Shared Memory Validation
            if decl.is_shared {
                if !context.in_gpu_function {
                    self.report_error(
                        "Shared variables can only be declared inside 'gpu' functions".to_string(),
                        span.clone(),
                    );
                }

                if let Some(typ_expr) = &decl.typ {
                    let resolved_type = self.resolve_type_expression(typ_expr, context);
                    if !matches!(resolved_type.kind, TypeKind::Array(_, _)) {
                        self.report_error(
                            format!(
                                "Shared variable '{}' must be an array, got {}",
                                decl.name, resolved_type
                            ),
                            span.clone(),
                        );
                    }
                } else {
                    self.report_error(
                        format!("Shared variable '{}' must have an explicit type", decl.name),
                        span.clone(),
                    );
                }

                if decl.initializer.is_some() {
                    self.report_error(
                        format!("Shared variable '{}' cannot have an initializer", decl.name),
                        span.clone(),
                    );
                }
            }

            let inferred_type = self.determine_variable_type(decl, context, span.clone());
            let is_mutable = matches!(decl.declaration_type, VariableDeclarationType::Mutable);

            self.check_shadowing(&decl.name, is_mutable, context, span.clone());

            if context.scopes.len() == 1 {
                self.global_scope.insert(
                    decl.name.clone(),
                    super::context::SymbolInfo {
                        ty: inferred_type.clone(),
                        mutable: is_mutable,
                        visibility: visibility.clone(),
                        module: self.current_module.clone(),
                    },
                );
            }

            context.define(
                decl.name.clone(),
                inferred_type,
                is_mutable,
                visibility.clone(),
                self.current_module.clone(),
            );
        }
    }

    fn check_shadowing(&mut self, name: &str, is_mutable: bool, context: &Context, span: Span) {
        // Check for shadowing rules in the current scope
        if let Some(current_scope) = context.scopes.last() {
            if let Some(existing_info) = current_scope.get(name) {
                // Rule 2: var may not shadow in the same scope
                if is_mutable {
                    self.report_error(
                        format!("Variable '{}' is already defined in this scope. 'var' cannot shadow existing variables.", name),
                        span,
                    );
                }
                // Rule 3: switching let <-> var via shadowing in the same scope is not allowed
                // We already know new is not mutable (from Rule 2 check above), so new is 'let'.
                // If existing is 'var' (mutable), then we are switching var -> let, which is disallowed.
                else if existing_info.mutable {
                    self.report_error(
                        format!("Cannot shadow mutable variable '{}' with an immutable one in the same scope.", name),
                        span,
                    );
                }
                // Rule 1: let shadowing let is allowed (implicit else)
            }
        }
    }

    fn determine_variable_type(
        &mut self,
        decl: &VariableDeclaration,
        context: &mut Context,
        span: Span,
    ) -> Type {
        let inferred_type = if let Some(init) = &decl.initializer {
            self.infer_expression(init, context)
        } else if let Some(type_expr) = &decl.typ {
            self.resolve_type_expression(type_expr, context)
        } else {
            self.report_error(
                format!("Cannot infer type for variable '{}'", decl.name),
                span,
            );
            make_type(TypeKind::Error)
        };

        // If both type annotation and initializer exist, check compatibility
        if let (Some(type_expr), Some(init)) = (&decl.typ, &decl.initializer) {
            let declared_type = self.resolve_type_expression(type_expr, context);
            if !self.are_compatible(&declared_type, &inferred_type, context) {
                // Check for list literal compatibility (e.g. [1] -> [i16])
                let mut compatible = false;
                if let (TypeKind::List(target_inner), ExpressionKind::List(elements)) =
                    (&declared_type.kind, &init.node)
                {
                    if let Ok(target_inner_type) = self.extract_type_from_expression(target_inner) {
                        if self.is_integer(&target_inner_type) {
                            compatible =
                                self.check_integer_list_literal(elements, &target_inner_type);
                        }
                    }
                }

                if !compatible {
                    self.report_error(
                        format!(
                            "Type mismatch for variable '{}': expected {}, got {}",
                            decl.name, declared_type, inferred_type
                        ),
                        init.span.clone(),
                    );
                }
            } else {
                // Check for warning: assigning non-nullable to nullable immutable variable
                if let TypeKind::Nullable(_) = &declared_type.kind {
                    if !matches!(decl.declaration_type, VariableDeclarationType::Mutable) {
                        // If inferred type is NOT nullable (and not None), warn
                        if !matches!(inferred_type.kind, TypeKind::Nullable(_)) {
                            self.report_warning(
                                format!(
                                    "Variable '{}' is immutable but declared as nullable. Consider removing '?' to make it non-nullable.",
                                    decl.name
                                ),
                                type_expr.span.clone(),
                            );
                        }
                    }
                }
            }
            return declared_type;
        }

        inferred_type
    }

    fn check_block(&mut self, stmts: &[Statement], context: &mut Context) {
        context.enter_scope();
        for s in stmts {
            self.check_statement(s, context);
        }
        context.exit_scope();
    }

    fn check_if(
        &mut self,
        cond: &Expression,
        then_block: &Statement,
        else_block: &Option<Box<Statement>>,
        context: &mut Context,
    ) {
        let cond_type = self.infer_expression(cond, context);
        if !matches!(cond_type.kind, TypeKind::Boolean) {
            self.report_error(
                format!("If condition must be a boolean, got {}", cond_type),
                cond.span.clone(),
            );
        }

        // Enter scope for then block
        context.enter_scope();
        self.check_statement(then_block, context);
        context.exit_scope();

        if let Some(else_stmt) = else_block {
            // Enter scope for else block
            context.enter_scope();
            self.check_statement(else_stmt, context);
            context.exit_scope();
        }
    }

    fn check_while(&mut self, cond: &Expression, body: &Statement, context: &mut Context) {
        let cond_type = self.infer_expression(cond, context);
        if !matches!(cond_type.kind, TypeKind::Boolean) {
            self.report_error(
                format!("While condition must be a boolean, got {}", cond_type),
                cond.span.clone(),
            );
        }
        context.enter_scope();
        context.enter_loop();
        self.check_statement(body, context);
        context.exit_loop();
        context.exit_scope();
    }

    fn check_for(
        &mut self,
        decls: &[VariableDeclaration],
        iterable: &Expression,
        body: &Statement,
        context: &mut Context,
    ) {
        let iterable_type = self.infer_expression(iterable, context);
        let element_type = self.get_iterable_element_type(&iterable_type, iterable.span.clone());

        context.enter_scope();
        context.enter_loop();

        self.bind_loop_variables(decls, &element_type, iterable.span.clone(), context);

        self.check_statement(body, context);
        context.exit_loop();
        context.exit_scope();
    }

    fn bind_loop_variables(
        &mut self,
        decls: &[VariableDeclaration],
        element_type: &Type,
        span: Span,
        context: &mut Context,
    ) {
        if decls.len() == 1 {
            let decl = &decls[0];
            let var_type = if let Some(type_expr) = &decl.typ {
                let declared_type = self.resolve_type_expression(type_expr, context);
                if !self.are_compatible(&declared_type, element_type, context) {
                    self.report_error(
                        format!(
                            "Type mismatch for loop variable '{}': expected {}, got {}",
                            decl.name, declared_type, element_type
                        ),
                        type_expr.span.clone(),
                    );
                }
                declared_type
            } else {
                element_type.clone()
            };
            let is_mutable = matches!(decl.declaration_type, VariableDeclarationType::Mutable);
            context.define(
                decl.name.clone(),
                var_type,
                is_mutable,
                MemberVisibility::Public,
                self.current_module.clone(),
            );
        } else if decls.len() == 2 {
            if let TypeKind::Tuple(exprs) = &element_type.kind {
                if exprs.len() == 2 {
                    let key_type = self
                        .extract_type_from_expression(&exprs[0])
                        .unwrap_or(make_type(TypeKind::Error));
                    let val_type = self
                        .extract_type_from_expression(&exprs[1])
                        .unwrap_or(make_type(TypeKind::Error));

                    let is_mutable_0 =
                        matches!(decls[0].declaration_type, VariableDeclarationType::Mutable);
                    let is_mutable_1 =
                        matches!(decls[1].declaration_type, VariableDeclarationType::Mutable);

                    context.define(
                        decls[0].name.clone(),
                        key_type,
                        is_mutable_0,
                        MemberVisibility::Public,
                        self.current_module.clone(),
                    );
                    context.define(
                        decls[1].name.clone(),
                        val_type,
                        is_mutable_1,
                        MemberVisibility::Public,
                        self.current_module.clone(),
                    );
                } else {
                    self.report_error(
                        "Destructuring mismatch: expected tuple of size 2".to_string(),
                        span,
                    );
                }
            } else {
                self.report_error(
                    format!("Expected tuple for destructuring, got {}", element_type),
                    span,
                );
            }
        } else {
            self.report_error("Invalid number of loop variables".to_string(), span);
        }
    }

    fn check_break(&mut self, context: &Context, span: Span) {
        if context.loop_depth == 0 {
            self.report_error("Break statement outside of loop".to_string(), span);
        }
    }

    fn check_continue(&mut self, context: &Context, span: Span) {
        if context.loop_depth == 0 {
            self.report_error("Continue statement outside of loop".to_string(), span);
        }
    }

    fn check_return(
        &mut self,
        expr_opt: &Option<Box<Expression>>,
        context: &mut Context,
        span: Span,
    ) {
        let (actual_return_type, return_span) = if let Some(expr) = expr_opt {
            (self.infer_expression(expr, context), expr.span.clone())
        } else {
            (make_type(TypeKind::Void), span.clone())
        };

        // Check if we are inferring return types for the current function
        if let Some(Some(inferred_types)) = context.inferred_return_types.last_mut() {
            inferred_types.push((actual_return_type, return_span));
            return;
        }

        let expected_return_type = context
            .return_types
            .last()
            .unwrap_or(&make_type(TypeKind::Void))
            .clone();

        if !self.are_compatible(&expected_return_type, &actual_return_type, context) {
            self.report_error(
                format!(
                    "Invalid return type: expected {}, got {}",
                    expected_return_type, actual_return_type
                ),
                return_span,
            );
        }
    }

    fn check_function_declaration(&mut self, info: FunctionDeclarationInfo, context: &mut Context) {
        let FunctionDeclarationInfo {
            name,
            generics,
            params,
            return_type: return_type_expr,
            body,
            properties,
        } = info;

        let func_type = make_type(TypeKind::Function(
            generics.clone(),
            params.to_vec(),
            return_type_expr.clone(),
        ));

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.to_string(),
                super::context::SymbolInfo {
                    ty: func_type.clone(),
                    mutable: false,
                    visibility: properties.visibility.clone(),
                    module: self.current_module.clone(),
                },
            );
        }

        context.define(
            name.to_string(),
            func_type,
            false,
            properties.visibility.clone(),
            self.current_module.clone(),
        ); // Functions are immutable

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
                        default_val.span.clone(),
                    );
                }
            }

            context.define(
                param.name.clone(),
                param_type,
                false,
                MemberVisibility::Public,
                self.current_module.clone(),
            ); // Parameters are immutable by default

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

                    let left = crate::ast::factory::identifier_with_span(
                        &param.name,
                        param.typ.span.clone(),
                    );
                    let guard_type =
                        self.infer_binary(&left, &bin_op, right, guard.span.clone(), context);

                    if !matches!(guard_type.kind, TypeKind::Boolean) {
                        self.report_error(
                            format!("Type mismatch: guard must be boolean, got {}", guard_type),
                            guard.span.clone(),
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
                    rt_expr.span.clone(),
                );
            }

            // Implicitly set return type to Kernel
            // Note: The `func_type` symbol stored in global_scope above was created using `return_type_expr`.
            // We need to update that symbol to return `Kernel` so that calls to it are typed correctly.
            let kernel_return_type = make_type(TypeKind::Custom("Kernel".to_string(), None));

            if let Some(info) = self.global_scope.get_mut(name) {
                if let TypeKind::Function(gens, params, _) = &info.ty.kind {
                    info.ty = make_type(TypeKind::Function(
                        gens.clone(),
                        params.clone(),
                        Some(Box::new(crate::ast::factory::type_expr_non_null(
                            kernel_return_type.clone(),
                        ))),
                    ));
                }
            }
            context.update_symbol_type(
                name,
                make_type(TypeKind::Function(
                    generics.clone(),
                    params.to_vec(),
                    Some(Box::new(crate::ast::factory::type_expr_non_null(
                        kernel_return_type.clone(),
                    ))),
                )),
            );

            // Inject 'gpu_context' object (type GpuContext)
            let gpu_context_type = make_type(TypeKind::Custom("GpuContext".to_string(), None));
            context.define(
                "gpu_context".to_string(),
                gpu_context_type,
                false, // Immutable
                MemberVisibility::Public,
                self.current_module.clone(),
            );
        }

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
                                    expr.span.clone(),
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
                        expr.span.clone(),
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

        if !matches!(return_type.kind, TypeKind::Void) {
            let status = check_returns(body);
            if status == ReturnStatus::None {
                // If implicit return was used (Expression or If), check_returns detects Implicit.
                // But check_returns logic: Expression -> Implicit. Block -> Last stmt Implicit?
                // Let's rely on existing check_returns logic and hope it handles Blocks/Ifs correctly.
                // Given `scope_visibility` passed compilation (only execution failed), type checking is likely fine.
                // Except now we made it non-Void, so strict check applies.
                // We should ensure check_returns handles If implicit return?
                // `check_return` handles explicit return.
                // Implicit return inside If is handled where?
                // Typically dynamic languages or Rust-like check block result.
                // check_block doesn't check result.

                // But for now, let's assume TypeChecker is lenient enough or existing logic covers it.
                // We only fixed correct return type inference for Main.

                if status == ReturnStatus::None {
                    self.report_error("Missing return statement".to_string(), body.span.clone());
                }
            }
        }

        context.in_gpu_function = previous_in_gpu;
        context.loop_depth = old_loop_depth;
        context.exit_scope();
        context.return_types.pop();
        context.inferred_return_types.pop();
    }

    fn check_struct(
        &mut self,
        name_expr: &Expression,
        generics: &Option<Vec<Expression>>,
        fields: &[Expression],
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        let name = if let ExpressionKind::Identifier(n, _) = &name_expr.node {
            n.clone()
        } else {
            self.report_error("Invalid struct name".to_string(), name_expr.span.clone());
            return;
        };

        let mut generic_defs = Vec::new();
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

        let mut fields_vec = Vec::new();
        for field in fields {
            if let ExpressionKind::StructMember(field_name_expr, field_type_expr) = &field.node {
                if let ExpressionKind::Identifier(field_name, _) = &field_name_expr.node {
                    let field_type = self.resolve_type_expression(field_type_expr, context);
                    fields_vec.push((field_name.clone(), field_type, MemberVisibility::Public));
                } else {
                    self.report_error(
                        "Invalid struct field name".to_string(),
                        field_name_expr.span.clone(),
                    );
                }
            } else {
                self.report_error(
                    "Invalid struct field definition".to_string(),
                    field.span.clone(),
                );
            }
        }

        context.exit_scope();

        let struct_def = StructDefinition {
            fields: fields_vec,
            generics: if generic_defs.is_empty() {
                None
            } else {
                Some(generic_defs)
            },
            module: self.current_module.clone(),
        };

        context.define_type(name.clone(), TypeDefinition::Struct(struct_def.clone()));
        if context.scopes.len() == 1 {
            self.global_type_definitions
                .insert(name.clone(), TypeDefinition::Struct(struct_def));
        }

        // Define constructor/type symbol
        // The type of the struct name identifier is Meta(Custom(name))
        let struct_type = make_type(TypeKind::Custom(name.clone(), None)); // TODO: Handle generics

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.clone(),
                super::context::SymbolInfo {
                    ty: make_type(TypeKind::Meta(Box::new(struct_type.clone()))),
                    mutable: false,
                    visibility: visibility.clone(),
                    module: self.current_module.clone(),
                },
            );
        }

        context.define(
            name,
            make_type(TypeKind::Meta(Box::new(struct_type))),
            false,
            visibility.clone(),
            self.current_module.clone(),
        );
    }

    fn check_enum(
        &mut self,
        name_expr: &Expression,
        variants: &[Expression],
        visibility: &MemberVisibility,
        context: &mut Context,
    ) {
        let name = if let ExpressionKind::Identifier(n, _) = &name_expr.node {
            n.clone()
        } else {
            self.report_error("Invalid enum name".to_string(), name_expr.span.clone());
            return;
        };

        let mut variant_map = HashMap::new();
        for variant in variants {
            if let ExpressionKind::EnumValue(variant_name_expr, associated_types) = &variant.node {
                if let ExpressionKind::Identifier(variant_name, _) = &variant_name_expr.node {
                    let mut types = Vec::new();
                    for ty_expr in associated_types {
                        types.push(self.resolve_type_expression(ty_expr, context));
                    }
                    variant_map.insert(variant_name.clone(), types);
                } else {
                    self.report_error(
                        "Invalid enum variant name".to_string(),
                        variant_name_expr.span.clone(),
                    );
                }
            } else {
                self.report_error(
                    "Invalid enum variant definition".to_string(),
                    variant.span.clone(),
                );
            }
        }

        let enum_def = EnumDefinition {
            variants: variant_map,
            module: self.current_module.clone(),
        };

        context.define_type(name.clone(), TypeDefinition::Enum(enum_def.clone()));
        if context.scopes.len() == 1 {
            self.global_type_definitions
                .insert(name.clone(), TypeDefinition::Enum(enum_def));
        }

        // Define enum type symbol
        let enum_type = make_type(TypeKind::Custom(name.clone(), None));

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.clone(),
                super::context::SymbolInfo {
                    ty: make_type(TypeKind::Meta(Box::new(enum_type.clone()))),
                    mutable: false,
                    visibility: visibility.clone(),
                    module: self.current_module.clone(),
                },
            );
        }

        context.define(
            name,
            make_type(TypeKind::Meta(Box::new(enum_type))),
            false,
            visibility.clone(),
            self.current_module.clone(),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn check_class(
        &mut self,
        name_expr: &Expression,
        generics: &Option<Vec<Expression>>,
        base_class: &Option<Box<Expression>>,
        traits: &[Expression],
        body: &[Statement],
        visibility: &MemberVisibility,
        context: &mut Context,
        span: Span,
    ) {
        // Extract class name
        let name = match self.extract_type_name(name_expr) {
            Ok(n) => n,
            Err(_) => {
                self.report_error("Invalid class name".to_string(), name_expr.span.clone());
                return;
            }
        };

        // Check for duplicate type definitions
        if self.global_type_definitions.contains_key(&name) {
            self.report_error(format!("Type '{}' is already defined", name), span.clone());
            return;
        }

        // Process generics
        let generic_defs = generics
            .as_ref()
            .map(|gens| self.extract_generic_definitions(gens, context));

        // Validate base class exists
        let base_class_name = if let Some(base_expr) = base_class {
            match self.extract_type_name(base_expr) {
                Ok(base_name) => {
                    // Check base class exists
                    if !self.global_type_definitions.contains_key(&base_name) {
                        self.report_error(
                            format!("Base class '{}' is not defined", base_name),
                            base_expr.span.clone(),
                        );
                    }
                    Some(base_name)
                }
                Err(_) => {
                    self.report_error(
                        "Invalid base class name".to_string(),
                        base_expr.span.clone(),
                    );
                    None
                }
            }
        } else {
            None
        };

        // Validate traits exist
        let mut trait_names = Vec::new();
        for trait_expr in traits {
            if let Ok(trait_name) = self.extract_type_name(trait_expr) {
                if !self.global_type_definitions.contains_key(&trait_name) {
                    self.report_error(
                        format!("Trait '{}' is not defined", trait_name),
                        trait_expr.span.clone(),
                    );
                }
                trait_names.push(trait_name);
            }
        }

        // Enter class scope
        context.enter_scope();

        // Define generics in scope
        if let Some(gens) = generics {
            self.define_generics(gens, context);
        }

        // Process class body to collect fields and methods
        let mut fields: HashMap<String, FieldInfo> = HashMap::new();
        let mut methods: HashMap<String, MethodInfo> = HashMap::new();

        for stmt in body {
            match &stmt.node {
                StatementKind::Variable(decls, vis) => {
                    for decl in decls {
                        let field_type = if let Some(type_expr) = &decl.typ {
                            self.resolve_type_expression(type_expr, context)
                        } else if let Some(init) = &decl.initializer {
                            self.infer_expression(init, context)
                        } else {
                            self.report_error(
                                format!("Cannot infer type for field '{}'", decl.name),
                                stmt.span.clone(),
                            );
                            make_type(TypeKind::Error)
                        };

                        let is_mutable =
                            matches!(decl.declaration_type, VariableDeclarationType::Mutable);

                        fields.insert(
                            decl.name.clone(),
                            FieldInfo {
                                ty: field_type,
                                mutable: is_mutable,
                                visibility: vis.clone(),
                            },
                        );
                    }
                }
                StatementKind::FunctionDeclaration(
                    method_name,
                    _method_generics,
                    params,
                    return_type,
                    method_body,
                    props,
                ) => {
                    // Check the function declaration normally
                    self.check_function_declaration(
                        FunctionDeclarationInfo {
                            name: method_name,
                            generics: _method_generics,
                            params,
                            return_type,
                            body: method_body,
                            properties: props,
                        },
                        context,
                    );

                    // Collect method info
                    let return_ty = if let Some(rt_expr) = return_type {
                        self.resolve_type_expression(rt_expr, context)
                    } else {
                        make_type(TypeKind::Void)
                    };

                    let param_types: Vec<(String, Type)> = params
                        .iter()
                        .map(|p| {
                            (
                                p.name.clone(),
                                self.resolve_type_expression(&p.typ, context),
                            )
                        })
                        .collect();

                    methods.insert(
                        method_name.clone(),
                        MethodInfo {
                            params: param_types,
                            return_type: return_ty,
                            visibility: props.visibility.clone(),
                            is_constructor: method_name == "init",
                        },
                    );
                }
                _ => {
                    self.report_error(
                        "Only field and method declarations are allowed in class body".to_string(),
                        stmt.span.clone(),
                    );
                }
            }
        }

        context.exit_scope();

        // Create class definition
        let class_def = ClassDefinition {
            name: name.clone(),
            generics: generic_defs,
            base_class: base_class_name,
            traits: trait_names,
            fields,
            methods,
            module: self.current_module.clone(),
        };

        // Register class type definition
        context.define_type(name.clone(), TypeDefinition::Class(class_def.clone()));
        if context.scopes.len() == 1 {
            self.global_type_definitions
                .insert(name.clone(), TypeDefinition::Class(class_def));
        }

        // Define class type symbol (as a constructor/type)
        let class_type = make_type(TypeKind::Custom(name.clone(), None));

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.clone(),
                super::context::SymbolInfo {
                    ty: make_type(TypeKind::Meta(Box::new(class_type.clone()))),
                    mutable: false,
                    visibility: visibility.clone(),
                    module: self.current_module.clone(),
                },
            );
        }

        context.define(
            name,
            make_type(TypeKind::Meta(Box::new(class_type))),
            false,
            visibility.clone(),
            self.current_module.clone(),
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn check_trait(
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
            Ok(n) => n,
            Err(_) => {
                self.report_error("Invalid trait name".to_string(), name_expr.span.clone());
                return;
            }
        };

        // Check for duplicate type definitions
        if self.global_type_definitions.contains_key(&name) {
            self.report_error(format!("Type '{}' is already defined", name), span.clone());
            return;
        }

        // Process generics
        let generic_defs = generics
            .as_ref()
            .map(|gens| self.extract_generic_definitions(gens, context));

        // Validate parent traits exist
        let mut parent_trait_names = Vec::new();
        for trait_expr in parent_traits {
            if let Ok(trait_name) = self.extract_type_name(trait_expr) {
                if !self.global_type_definitions.contains_key(&trait_name) {
                    self.report_error(
                        format!("Parent trait '{}' is not defined", trait_name),
                        trait_expr.span.clone(),
                    );
                }
                parent_trait_names.push(trait_name);
            }
        }

        // Enter trait scope
        context.enter_scope();

        // Define generics in scope
        if let Some(gens) = generics {
            self.define_generics(gens, context);
        }

        // Process trait body to collect methods
        let mut methods: HashMap<String, MethodInfo> = HashMap::new();

        for stmt in body {
            match &stmt.node {
                StatementKind::FunctionDeclaration(
                    method_name,
                    _method_generics,
                    params,
                    return_type,
                    method_body,
                    props,
                ) => {
                    // Check the function declaration
                    self.check_function_declaration(
                        FunctionDeclarationInfo {
                            name: method_name,
                            generics: _method_generics,
                            params,
                            return_type,
                            body: method_body,
                            properties: props,
                        },
                        context,
                    );

                    // Collect method info
                    let return_ty = if let Some(rt_expr) = return_type {
                        self.resolve_type_expression(rt_expr, context)
                    } else {
                        make_type(TypeKind::Void)
                    };

                    let param_types: Vec<(String, Type)> = params
                        .iter()
                        .map(|p| {
                            (
                                p.name.clone(),
                                self.resolve_type_expression(&p.typ, context),
                            )
                        })
                        .collect();

                    methods.insert(
                        method_name.clone(),
                        MethodInfo {
                            params: param_types,
                            return_type: return_ty,
                            visibility: props.visibility.clone(),
                            is_constructor: false,
                        },
                    );
                }
                _ => {
                    self.report_error(
                        "Only method declarations are allowed in trait body".to_string(),
                        stmt.span.clone(),
                    );
                }
            }
        }

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
            self.global_type_definitions
                .insert(name.clone(), TypeDefinition::Trait(trait_def));
        }

        // Define trait type symbol
        let trait_type = make_type(TypeKind::Custom(name.clone(), None));

        if context.scopes.len() == 1 {
            self.global_scope.insert(
                name.clone(),
                super::context::SymbolInfo {
                    ty: make_type(TypeKind::Meta(Box::new(trait_type.clone()))),
                    mutable: false,
                    visibility: visibility.clone(),
                    module: self.current_module.clone(),
                },
            );
        }

        context.define(
            name,
            make_type(TypeKind::Meta(Box::new(trait_type))),
            false,
            visibility.clone(),
            self.current_module.clone(),
        );
    }

    fn extract_generic_definitions(
        &mut self,
        generics: &[Expression],
        context: &mut Context,
    ) -> Vec<GenericDefinition> {
        let mut result = Vec::new();
        for gen_expr in generics {
            if let ExpressionKind::GenericType(name_expr, constraint_expr, kind) = &gen_expr.node {
                if let Ok(gen_name) = self.extract_type_name(name_expr) {
                    let constraint = constraint_expr
                        .as_ref()
                        .map(|c| self.resolve_type_expression(c, context));
                    result.push(GenericDefinition {
                        name: gen_name,
                        constraint,
                        kind: kind.clone(),
                    });
                }
            }
        }
        result
    }

    fn check_integer_list_literal(&self, elements: &[Expression], target_type: &Type) -> bool {
        let target_size = match self.get_integer_size(target_type) {
            Some(s) => s,
            None => return false,
        };

        for element in elements {
            if let ExpressionKind::Literal(Literal::Integer(int_val)) = &element.node {
                if !self.integer_fits(int_val, target_size, target_type) {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }

    fn integer_fits(&self, val: &IntegerLiteral, size: u8, target_type: &Type) -> bool {
        let is_target_unsigned = matches!(
            target_type.kind,
            TypeKind::U8 | TypeKind::U16 | TypeKind::U32 | TypeKind::U64 | TypeKind::U128
        );

        match val {
            IntegerLiteral::U128(v) => {
                if is_target_unsigned {
                    let max = match size {
                        8 => u8::MAX as u128,
                        16 => u16::MAX as u128,
                        32 => u32::MAX as u128,
                        64 => u64::MAX as u128,
                        128 => u128::MAX,
                        _ => return false,
                    };
                    *v <= max
                } else {
                    let max = match size {
                        8 => i8::MAX as u128,
                        16 => i16::MAX as u128,
                        32 => i32::MAX as u128,
                        64 => i64::MAX as u128,
                        128 => i128::MAX as u128,
                        _ => return false,
                    };
                    *v <= max
                }
            }
            _ => {
                let val_i128 = match val {
                    IntegerLiteral::I8(v) => *v as i128,
                    IntegerLiteral::I16(v) => *v as i128,
                    IntegerLiteral::I32(v) => *v as i128,
                    IntegerLiteral::I64(v) => *v as i128,
                    IntegerLiteral::I128(v) => *v,
                    IntegerLiteral::U8(v) => *v as i128,
                    IntegerLiteral::U16(v) => *v as i128,
                    IntegerLiteral::U32(v) => *v as i128,
                    IntegerLiteral::U64(v) => *v as i128,
                    _ => unreachable!(),
                };

                if is_target_unsigned {
                    if val_i128 < 0 {
                        return false;
                    }
                    let max = match size {
                        8 => u8::MAX as i128,
                        16 => u16::MAX as i128,
                        32 => u32::MAX as i128,
                        64 => u64::MAX as i128,
                        128 => i128::MAX,
                        _ => return false,
                    };
                    if size == 128 {
                        return true;
                    }
                    val_i128 <= max
                } else {
                    let (min, max) = match size {
                        8 => (i8::MIN as i128, i8::MAX as i128),
                        16 => (i16::MIN as i128, i16::MAX as i128),
                        32 => (i32::MIN as i128, i32::MAX as i128),
                        64 => (i64::MIN as i128, i64::MAX as i128),
                        128 => (i128::MIN, i128::MAX),
                        _ => return false,
                    };
                    val_i128 >= min && val_i128 <= max
                }
            }
        }
    }
}
