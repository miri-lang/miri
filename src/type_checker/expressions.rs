// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression type inference for the type checker.
//!
//! This module implements type inference for all expression kinds in Miri.
//! The main entry point is [`TypeChecker::infer_expression`], which dispatches
//! to specialized inference methods based on the expression kind.
//!
//! # Supported Expressions
//!
//! ## Literals
//! - Integer, float, string, boolean, and none literals
//!
//! ## Operators
//! - Binary: arithmetic (`+`, `-`, `*`, `/`, `%`), comparison (`<`, `>`, `==`, etc.)
//! - Logical: `and`, `or`
//! - Unary: `-`, `+`, `not`, `~`, `await`
//!
//! ## Collections
//! - Lists: `[1, 2, 3]` → `List<int>`
//! - Maps: `{"a": 1}` → `Map<string, int>`
//! - Sets: `{1, 2, 3}` → `Set<int>`
//! - Tuples: `(1, "a")` → `(int, string)`
//! - Ranges: `1..10` → `Range<int>`
//!
//! ## Access
//! - Member access: `obj.field`
//! - Index access: `list[0]`, `map["key"]`
//!
//! ## Functions
//! - Function calls with generic type inference
//! - Lambda expressions with type inference
//! - Method calls on objects
//!
//! ## Control Flow
//! - Conditional expressions: `x if cond else y`
//! - Match expressions with pattern matching
//!
//! ## Types
//! - Struct instantiation: `Point { x: 1, y: 2 }`
//! - Enum variant construction: `Ok(value)`, `Err(error)`
//! - Generic type instantiation

use super::context::{Context, TypeDefinition};
use super::TypeChecker;
use crate::ast::factory as ast_factory;
use crate::ast::factory::make_type;
use crate::ast::types::{Type, TypeDeclarationKind, TypeKind};
use crate::ast::*;
use crate::error::format::find_best_match;
use crate::error::syntax::Span;
use std::collections::{HashMap, HashSet};

impl TypeChecker {
    /// Infers the type of an expression.
    ///
    /// This is the main entry point for expression type checking. It delegates to specific
    /// handler methods based on the expression kind.
    pub(crate) fn infer_expression(&mut self, expr: &Expression, context: &mut Context) -> Type {
        let ty = match &expr.node {
            ExpressionKind::Literal(lit) => self.infer_literal(lit),
            ExpressionKind::Binary(left, op, right) => {
                self.infer_binary(left, op, right, expr.span.clone(), context)
            }
            ExpressionKind::Logical(left, op, right) => {
                self.infer_logical(left, op, right, expr.span.clone(), context)
            }
            ExpressionKind::Unary(op, operand) => {
                self.infer_unary(op, operand, expr.span.clone(), context)
            }
            ExpressionKind::Identifier(name, _) => {
                self.infer_identifier(name, expr.span.clone(), context)
            }
            ExpressionKind::Assignment(lhs, op, rhs) => {
                self.infer_assignment(lhs, op, rhs, expr.span.clone(), context)
            }
            ExpressionKind::Call(func, args) => {
                self.infer_call(func, args, expr.span.clone(), context)
            }
            ExpressionKind::Range(start, end, kind) => {
                self.infer_range(start, end, kind, expr.span.clone(), context)
            }
            ExpressionKind::List(elements) => self.infer_list(elements, context),
            ExpressionKind::Map(entries) => self.infer_map(entries, context),
            ExpressionKind::Set(elements) => self.infer_set(elements, context),
            ExpressionKind::Tuple(elements) => self.infer_tuple(elements, context),
            ExpressionKind::Index(obj, index) => {
                self.infer_index(obj, index, expr.span.clone(), context)
            }
            ExpressionKind::Member(obj, prop) => {
                self.infer_member(obj, prop, expr.span.clone(), context)
            }
            ExpressionKind::Match(subject, branches) => {
                self.infer_match(subject, branches, expr.span.clone(), context)
            }
            ExpressionKind::Conditional(then_expr, cond_expr, else_expr, _) => {
                self.infer_conditional(then_expr, cond_expr, else_expr, expr.span.clone(), context)
            }
            ExpressionKind::FormattedString(parts) => self.infer_formatted_string(parts, context),
            ExpressionKind::Lambda(generics, params, return_type, body, properties) => {
                self.infer_lambda(generics, params, return_type, body, properties, context)
            }
            ExpressionKind::TypeDeclaration(expr, generics, kind, target) => self
                .infer_generic_instantiation(
                    expr,
                    generics,
                    kind,
                    target,
                    expr.span.clone(),
                    context,
                ),
            ExpressionKind::NamedArgument(_, value) => self.infer_expression(value, context),
            ExpressionKind::EnumValue(name, values) => {
                self.infer_enum_value(name, values, expr.span.clone(), context)
            }
            ExpressionKind::Super => self.infer_super(expr.span.clone(), context),
            ExpressionKind::Block(statements, final_expr) => {
                // Type check all statements, then the final expression determines the type
                for stmt in statements {
                    self.infer_statement_type(stmt, context);
                }
                self.infer_expression(final_expr, context)
            }
            _ => ast_factory::make_type(TypeKind::Int), // Default fallback for unimplemented expressions
        };

        self.types.insert(expr.id, ty.clone());
        ty
    }

    fn infer_enum_value(
        &mut self,
        name: &Expression,
        values: &[Expression],
        span: Span,
        context: &mut Context,
    ) -> Type {
        if let ExpressionKind::Identifier(id_name, _) = &name.node {
            if id_name == "Ok" {
                if values.len() != 1 {
                    self.report_error("Ok expects exactly 1 argument".to_string(), span);
                    return ast_factory::make_type(TypeKind::Error);
                }
                let val_type = self.infer_expression(&values[0], context);
                // result<T, Void>
                return ast_factory::make_type(TypeKind::Result(
                    Box::new(ast_factory::expr_with_span(
                        ExpressionKind::Type(Box::new(val_type), false),
                        span.clone(),
                    )),
                    Box::new(ast_factory::expr_with_span(
                        ExpressionKind::Type(
                            Box::new(ast_factory::make_type(TypeKind::Void)),
                            false,
                        ),
                        span.clone(),
                    )),
                ));
            } else if id_name == "Err" {
                if values.len() != 1 {
                    self.report_error("Err expects exactly 1 argument".to_string(), span);
                    return ast_factory::make_type(TypeKind::Error);
                }
                let val_type = self.infer_expression(&values[0], context);
                // result<Void, E>
                return ast_factory::make_type(TypeKind::Result(
                    Box::new(ast_factory::expr_with_span(
                        ExpressionKind::Type(
                            Box::new(ast_factory::make_type(TypeKind::Void)),
                            false,
                        ),
                        span.clone(),
                    )),
                    Box::new(ast_factory::expr_with_span(
                        ExpressionKind::Type(Box::new(val_type), false),
                        span.clone(),
                    )),
                ));
            }
        }

        // Handle user-defined enums with Member access (e.g., Color.Red(args))
        if let ExpressionKind::Member(enum_name_expr, variant_name_expr) = &name.node {
            if let (
                ExpressionKind::Identifier(enum_name, _),
                ExpressionKind::Identifier(variant_name, _),
            ) = (&enum_name_expr.node, &variant_name_expr.node)
            {
                // Look up the enum definition in local then global scope
                let enum_def_opt = context
                    .resolve_type_definition(enum_name)
                    .cloned()
                    .or_else(|| self.global_type_definitions.get(enum_name).cloned());

                if let Some(TypeDefinition::Enum(enum_def)) = enum_def_opt {
                    if let Some(variant_types) = enum_def.variants.get(variant_name) {
                        // Check argument count
                        if values.len() != variant_types.len() {
                            self.report_error(
                                format!(
                                    "Enum variant '{}.{}' expects {} arguments, got {}",
                                    enum_name,
                                    variant_name,
                                    variant_types.len(),
                                    values.len()
                                ),
                                span.clone(),
                            );
                            return ast_factory::make_type(TypeKind::Error);
                        }

                        // Type-check each argument against the variant's types
                        // Build generic mapping if the enum is generic
                        let generic_mapping: HashMap<String, Type> = if let Some(ref generics) =
                            enum_def.generics
                        {
                            // Try to infer generic args from the arguments
                            let mut mapping = HashMap::new();
                            for (val, var_type) in values.iter().zip(variant_types.iter()) {
                                let val_type = self.infer_expression(val, context);
                                if let TypeKind::Generic(name, _, _) = &var_type.kind {
                                    mapping.insert(name.clone(), val_type);
                                }
                            }
                            // Fill in remaining generics with Error type
                            for g in generics {
                                mapping
                                    .entry(g.name.clone())
                                    .or_insert_with(|| ast_factory::make_type(TypeKind::Error));
                            }
                            mapping
                        } else {
                            // Non-generic: just type-check arguments directly
                            for (val, var_type) in values.iter().zip(variant_types.iter()) {
                                let val_type = self.infer_expression(val, context);
                                if !self.are_compatible(&val_type, var_type, context) {
                                    self.report_error(
                                            format!(
                                                "Type mismatch in enum variant '{}.{}': expected {}, got {}",
                                                enum_name, variant_name, var_type, val_type
                                            ),
                                            val.span.clone(),
                                        );
                                }
                            }
                            HashMap::new()
                        };

                        // For generic enums, also validate inferred args against variant types
                        if enum_def.generics.is_some() && !generic_mapping.is_empty() {
                            for (val, var_type) in values.iter().zip(variant_types.iter()) {
                                let val_type = self.infer_expression(val, context);
                                let substituted = self.substitute_type(var_type, &generic_mapping);
                                if !self.are_compatible(&val_type, &substituted, context) {
                                    self.report_error(
                                        format!(
                                            "Type mismatch in enum variant '{}.{}': expected {}, got {}",
                                            enum_name, variant_name, substituted, val_type
                                        ),
                                        val.span.clone(),
                                    );
                                }
                            }
                        }

                        // Build generic args for the return type
                        let generic_args = if let Some(ref generics) = enum_def.generics {
                            let args: Vec<Expression> = generics
                                .iter()
                                .map(|g| {
                                    let ty = generic_mapping
                                        .get(&g.name)
                                        .cloned()
                                        .unwrap_or_else(|| ast_factory::make_type(TypeKind::Error));
                                    self.create_type_expression(ty)
                                })
                                .collect();
                            Some(args)
                        } else {
                            None
                        };

                        return ast_factory::make_type(TypeKind::Custom(
                            enum_name.clone(),
                            generic_args,
                        ));
                    } else {
                        self.report_error(
                            format!("Enum '{}' has no variant '{}'", enum_name, variant_name),
                            span,
                        );
                        return ast_factory::make_type(TypeKind::Error);
                    }
                } else {
                    self.report_error(format!("'{}' is not an Enum", enum_name), span);
                    return ast_factory::make_type(TypeKind::Error);
                }
            }
        }

        ast_factory::make_type(TypeKind::Error)
    }

    fn infer_literal(&self, lit: &Literal) -> Type {
        match lit {
            Literal::Integer(_) => ast_factory::make_type(TypeKind::Int),
            Literal::Float(f) => match f {
                FloatLiteral::F32(_) => ast_factory::make_type(TypeKind::F32),
                FloatLiteral::F64(_) => ast_factory::make_type(TypeKind::F64),
            },
            Literal::Boolean(_) => ast_factory::make_type(TypeKind::Boolean),
            Literal::String(_) => ast_factory::make_type(TypeKind::String),
            Literal::Symbol(_) => ast_factory::make_type(TypeKind::Symbol),
            Literal::Regex(_) => ast_factory::make_type(TypeKind::Custom("Regex".into(), None)),
            Literal::None => ast_factory::make_type(TypeKind::Nullable(Box::new(
                ast_factory::make_type(TypeKind::Void),
            ))),
        }
    }

    /// Infers the type of a 'self' expression.
    ///
    /// `self` refers to the current class instance. It can only be used inside a class method.
    fn infer_self(&mut self, span: Span, context: &Context) -> Type {
        if let Some(class_type) = &context.current_class_type {
            class_type.clone()
        } else {
            self.report_error(
                "'self' can only be used inside a class method".to_string(),
                span,
            );
            ast_factory::make_type(TypeKind::Error)
        }
    }

    /// Infers the type of a 'super' expression.
    ///
    /// `super` refers to the parent class. It can only be used inside a class that extends another.
    fn infer_super(&mut self, span: Span, context: &Context) -> Type {
        if context.current_class.is_none() {
            self.report_error(
                "'super' can only be used inside a class method".to_string(),
                span,
            );
            return ast_factory::make_type(TypeKind::Error);
        }

        if let Some(base_class) = &context.current_base_class {
            ast_factory::make_type(TypeKind::Custom(base_class.clone(), None))
        } else {
            self.report_error(
                "'super' can only be used in a class that extends another class".to_string(),
                span,
            );
            ast_factory::make_type(TypeKind::Error)
        }
    }

    /// Infers the type of a binary operation.
    ///
    /// Checks compatibility of operands and determines the result type.
    pub(crate) fn infer_binary(
        &mut self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let left_ty = self.infer_expression(left, context);
        let right_ty = self.infer_expression(right, context);

        if matches!(op, BinaryOp::Div | BinaryOp::Mod) {
            let is_zero = match &right.node {
                ExpressionKind::Literal(lit) => lit.is_zero(),
                ExpressionKind::Unary(UnaryOp::Negate | UnaryOp::Plus, operand) => {
                    matches!(&operand.node, ExpressionKind::Literal(lit) if lit.is_zero())
                }
                _ => false,
            };
            if is_zero {
                self.report_error("Division by zero".to_string(), right.span.clone());
                return ast_factory::make_type(TypeKind::Error);
            }
        }

        match self.check_binary_op_types(&left_ty, op, &right_ty, context) {
            Ok(t) => t,
            Err(msg) => {
                self.report_error(msg, span);
                ast_factory::make_type(TypeKind::Error)
            }
        }
    }

    fn infer_logical(
        &mut self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        self.infer_binary(left, op, right, span, context)
    }

    fn infer_unary(
        &mut self,
        op: &UnaryOp,
        operand: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        // Check for double negation pattern (--x)
        if matches!(op, UnaryOp::Negate) {
            if let ExpressionKind::Unary(UnaryOp::Negate, _) = &operand.node {
                self.report_warning("use of a double negation".to_string(), span.clone());
            }
        } else if matches!(op, UnaryOp::Decrement) {
            self.report_warning("use of a double negation".to_string(), span.clone());
        }

        // Validate await context: allowed outside functions or inside async functions
        if matches!(op, UnaryOp::Await) && context.in_function && !context.in_async_function {
            self.report_error(
                "'await' can only be used in async functions or at the top level".to_string(),
                span.clone(),
            );
            return ast_factory::make_type(TypeKind::Error);
        }

        let expr_ty = self.infer_expression(operand, context);
        match self.check_unary_op_types(op, &expr_ty) {
            Ok(t) => t,
            Err(msg) => {
                self.report_error(msg, span);
                ast_factory::make_type(TypeKind::Error)
            }
        }
    }

    fn infer_identifier(&mut self, name: &str, span: Span, context: &mut Context) -> Type {
        // println!("Inferring identifier: {}", name);
        if name == "None" {
            return ast_factory::make_type(TypeKind::Nullable(Box::new(ast_factory::make_type(
                TypeKind::Void,
            ))));
        }
        if name == "Ok" {
            // fn<T>(value T): result<T, Void>
            let t_param = ast_factory::make_type(TypeKind::Generic(
                "T".to_string(),
                None,
                TypeDeclarationKind::None,
            ));
            let t_expr = ast_factory::type_expr_non_null(t_param.clone());
            let void_expr = ast_factory::type_expr_non_null(ast_factory::make_type(TypeKind::Void));

            let return_type = ast_factory::make_type(TypeKind::Result(
                Box::new(t_expr.clone()),
                Box::new(void_expr),
            ));

            return ast_factory::make_type(TypeKind::Function(
                Some(vec![t_expr.clone()]),
                vec![Parameter {
                    name: "value".to_string(),
                    typ: Box::new(t_expr),
                    guard: None,
                    default_value: None,
                }],
                Some(Box::new(ast_factory::type_expr_non_null(return_type))),
            ));
        }
        if name == "Err" {
            // fn<E>(error E): result<Void, E>
            let e_param = ast_factory::make_type(TypeKind::Generic(
                "E".to_string(),
                None,
                TypeDeclarationKind::None,
            ));
            let e_expr = ast_factory::type_expr_non_null(e_param.clone());
            let void_expr = ast_factory::type_expr_non_null(ast_factory::make_type(TypeKind::Void));

            let return_type = ast_factory::make_type(TypeKind::Result(
                Box::new(void_expr),
                Box::new(e_expr.clone()),
            ));

            return ast_factory::make_type(TypeKind::Function(
                Some(vec![e_expr.clone()]),
                vec![Parameter {
                    name: "error".to_string(),
                    typ: Box::new(e_expr),
                    guard: None,
                    default_value: None,
                }],
                Some(Box::new(ast_factory::type_expr_non_null(return_type))),
            ));
        }

        // Handle 'self' keyword - refers to current class instance
        if name == "self" {
            return self.infer_self(span, context);
        }

        let info_opt = context
            .resolve_info(name)
            .or_else(|| self.global_scope.get(name).cloned());

        if let Some(info) = info_opt {
            if !self.check_visibility(&info.visibility, &info.module) {
                self.report_error(format!("Variable '{}' is not visible", name), span);
                return ast_factory::make_type(TypeKind::Error);
            }

            // Linearity Check: Ensure linear resources are used exactly once
            if let TypeKind::Linear(_) = &info.ty.kind {
                if context.mark_consumed(name) {
                    self.report_error(format!("Use of moved value: '{}'", name), span);
                    return ast_factory::make_type(TypeKind::Error);
                }
            }

            return info.ty;
        }

        // Check if it is a known type (struct/enum/alias) being used as a value (constructor/meta)
        if self.global_type_definitions.contains_key(name) {
            return ast_factory::make_type(TypeKind::Meta(Box::new(ast_factory::make_type(
                TypeKind::Custom(name.to_string(), None),
            ))));
        }

        let mut candidates: Vec<&str> = Vec::new();
        for scope in &context.scopes {
            candidates.extend(scope.keys().map(|s| s.as_str()));
        }
        candidates.extend(self.global_scope.keys().map(|s| s.as_str()));
        candidates.push("None");
        candidates.push("Ok");
        candidates.push("Err");

        if let Some(suggestion) = find_best_match(name, &candidates) {
            self.report_error_with_help(
                format!("Undefined variable: {}", name),
                span,
                format!("Did you mean '{}'?", suggestion),
            );
        } else {
            self.report_error(format!("Undefined variable: {}", name), span);
        }
        ast_factory::make_type(TypeKind::Error)
    }

    fn infer_assignment(
        &mut self,
        lhs: &LeftHandSideExpression,
        op: &AssignmentOp,
        rhs: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let rhs_type = self.infer_expression(rhs, context);
        let lhs_type = match lhs {
            LeftHandSideExpression::Identifier(id_expr) => {
                if let ExpressionKind::Identifier(name, _) = &id_expr.node {
                    // 'self' is always mutable in class context (for constructor assignment)
                    if name != "self" && !context.is_mutable(name) {
                        self.report_error(
                            format!("Cannot assign to immutable variable '{}'", name),
                            span.clone(),
                        );
                    }
                    self.infer_identifier(name, id_expr.span.clone(), context)
                } else {
                    self.report_error("Invalid assignment target".to_string(), span.clone());
                    ast_factory::make_type(TypeKind::Error)
                }
            }
            LeftHandSideExpression::Member(member_expr) => {
                if let ExpressionKind::Member(obj, prop) = &member_expr.node {
                    if !self.is_mutable_expression(obj, context) {
                        self.report_error(
                            "Cannot assign to field of immutable variable".to_string(),
                            span.clone(),
                        );
                    }
                    self.infer_member(obj, prop, member_expr.span.clone(), context)
                } else {
                    ast_factory::make_type(TypeKind::Error)
                }
            }
            LeftHandSideExpression::Index(index_expr) => {
                if let ExpressionKind::Index(obj, index) = &index_expr.node {
                    if !self.is_mutable_expression(obj, context) {
                        self.report_error(
                            "Cannot assign to element of immutable variable".to_string(),
                            span.clone(),
                        );
                    }
                    self.infer_index(obj, index, index_expr.span.clone(), context)
                } else {
                    ast_factory::make_type(TypeKind::Error)
                }
            }
        };

        if matches!(op, AssignmentOp::AssignDiv | AssignmentOp::AssignMod) {
            if let ExpressionKind::Literal(lit) = &rhs.node {
                if lit.is_zero() {
                    self.report_error("Division by zero".to_string(), rhs.span.clone());
                }
            }
        }

        if !self.are_compatible(&lhs_type, &rhs_type, context) {
            self.report_error(
                format!(
                    "Type mismatch in assignment: cannot assign {} to {}",
                    rhs_type, lhs_type
                ),
                span.clone(),
            );
        }

        lhs_type
    }

    fn infer_call(
        &mut self,
        func: &Expression,
        args: &[Expression],
        span: Span,
        context: &mut Context,
    ) -> Type {
        let func_type = self.infer_expression(func, context);

        // Process arguments
        let mut positional_args = Vec::new();
        let mut named_args = HashMap::new();

        for arg in args {
            match &arg.node {
                ExpressionKind::NamedArgument(name, value) => {
                    if named_args.contains_key(name) {
                        self.report_error(
                            format!("Duplicate argument '{}'", name),
                            arg.span.clone(),
                        );
                    } else {
                        let ty = self.infer_expression(value, context);
                        named_args.insert(name.clone(), (value, ty, arg.span.clone()));
                    }
                }
                _ => {
                    if !named_args.is_empty() {
                        self.report_error(
                            "Positional arguments cannot follow named arguments".to_string(),
                            arg.span.clone(),
                        );
                    }
                    let ty = self.infer_expression(arg, context);
                    positional_args.push((arg, ty));
                }
            }
        }

        match &func_type.kind {
            TypeKind::Function(generics, params, return_type_expr) => {
                let mut generic_map = std::collections::HashMap::new();

                if let Some(gens) = &generics {
                    context.enter_scope();
                    self.define_generics(gens, context);
                }

                let mut pos_iter = positional_args.iter();

                for param in params {
                    let param_type = self.resolve_type_expression(&param.typ, context);

                    let (arg_expr, arg_type) = if let Some((expr, ty)) = pos_iter.next() {
                        (Some(*expr), Some(ty.clone()))
                    } else if let Some((expr, ty, _)) = named_args.remove(&param.name) {
                        (Some(&**expr), Some(ty))
                    } else {
                        (None, None)
                    };

                    if let Some(arg_type) = arg_type {
                        if generics.is_some() {
                            self.infer_generic_types(&param_type, &arg_type, &mut generic_map);
                        }

                        let concrete_param_type = if generics.is_some() {
                            self.substitute_type(&param_type, &generic_map)
                        } else {
                            param_type.clone()
                        };

                        if !self.are_compatible(&concrete_param_type, &arg_type, context) {
                            self.report_error(
                                format!(
                                    "Type mismatch for argument '{}': expected {}, got {}",
                                    param.name, concrete_param_type, arg_type
                                ),
                                arg_expr.map(|e| e.span.clone()).unwrap_or(span.clone()),
                            );
                        }
                    } else if param.default_value.is_none() {
                        self.report_error(
                            format!("Missing argument for parameter '{}'", param.name),
                            span.clone(),
                        );
                    }
                }

                if pos_iter.next().is_some() {
                    self.report_error(
                        format!(
                            "Too many positional arguments: expected {}, got {}",
                            params.len(),
                            positional_args.len()
                        ),
                        span.clone(),
                    );
                }

                for (name, (_, _, span)) in named_args {
                    self.report_error(format!("Unknown argument '{}'", name), span);
                }

                let return_type = if let Some(rt_expr) = return_type_expr {
                    let rt = self.resolve_type_expression(rt_expr, context);
                    if generics.is_some() {
                        self.substitute_type(&rt, &generic_map)
                    } else {
                        rt
                    }
                } else {
                    ast_factory::make_type(TypeKind::Void)
                };

                if generics.is_some() {
                    context.exit_scope();
                }

                // GPU kernels cannot call host functions.
                if context.in_gpu_function {
                    if let ExpressionKind::Identifier(name, _) = &func.node {
                        if name == "print" {
                            self.report_error(
                                "Host function 'print' cannot be called from a GPU kernel"
                                    .to_string(),
                                span,
                            );
                        }
                    }
                }

                return_type
            }
            TypeKind::Meta(inner_type) => {
                if let TypeKind::Custom(name, _) = &inner_type.kind {
                    let type_def = context
                        .resolve_type_definition(name)
                        .cloned()
                        .or_else(|| self.global_type_definitions.get(name).cloned());

                    // Check for Class constructor
                    if let Some(TypeDefinition::Class(def)) = &type_def {
                        // Prevent instantiation of abstract classes
                        if def.is_abstract {
                            self.report_error(
                                format!(
                                    "Cannot instantiate abstract class '{}'. Abstract classes cannot be instantiated directly.",
                                    name
                                ),
                                span.clone(),
                            );
                            return make_type(TypeKind::Error);
                        }

                        // Class constructors are handled via init method
                        // For now, just return the class type
                        return make_type(TypeKind::Custom(name.clone(), None));
                    }

                    if let Some(TypeDefinition::Struct(def)) = type_def {
                        let mut pos_iter = positional_args.iter();
                        let mut generic_map = HashMap::new();

                        for (field_name, field_type, _) in &def.fields {
                            let (arg_expr, arg_type) = if let Some((expr, ty)) = pos_iter.next() {
                                (Some(*expr), Some(ty.clone()))
                            } else if let Some((expr, ty, _)) = named_args.remove(field_name) {
                                (Some(&**expr), Some(ty))
                            } else {
                                (None, None)
                            };

                            if let Some(arg_type) = arg_type {
                                if def.generics.is_some() {
                                    self.infer_generic_types(
                                        field_type,
                                        &arg_type,
                                        &mut generic_map,
                                    );
                                }

                                let concrete_field_type = if def.generics.is_some() {
                                    self.substitute_type(field_type, &generic_map)
                                } else {
                                    field_type.clone()
                                };

                                if !self.are_compatible(&concrete_field_type, &arg_type, context) {
                                    self.report_error(
                                        format!(
                                            "Type mismatch for field '{}': expected {}, got {}",
                                            field_name, concrete_field_type, arg_type
                                        ),
                                        arg_expr.map(|e| e.span.clone()).unwrap_or(span.clone()),
                                    );
                                }
                            } else {
                                self.report_error(
                                    format!("Missing argument for field '{}'", field_name),
                                    span.clone(),
                                );
                            }
                        }

                        if pos_iter.next().is_some() {
                            self.report_error(
                                format!(
                                    "Too many positional arguments for struct constructor: expected {}, got {}",
                                    def.fields.len(),
                                    positional_args.len()
                                ),
                                span.clone(),
                            );
                        }

                        for (name, (_, _, span)) in named_args {
                            self.report_error(format!("Unknown field '{}'", name), span);
                        }

                        let generic_args = def.generics.as_ref().map(|gens| {
                            gens.iter()
                                .map(|g| {
                                    generic_map
                                        .get(&g.name)
                                        .cloned()
                                        .unwrap_or(make_type(TypeKind::Error))
                                })
                                .map(|t| self.create_type_expression(t))
                                .collect()
                        });

                        return make_type(TypeKind::Custom(name.clone(), generic_args));
                    }
                }
                self.report_error(format!("Type '{}' is not callable", inner_type), span);
                make_type(TypeKind::Error)
            }
            _ if matches!(func_type.kind, TypeKind::Error) => make_type(TypeKind::Error),
            _ => {
                self.report_error(
                    format!("Expression is not callable: {}", func_type),
                    func.span.clone(),
                );
                make_type(TypeKind::Error)
            }
        }
    }

    fn infer_range(
        &mut self,
        start: &Expression,
        end: &Option<Box<Expression>>,
        kind: &RangeExpressionType,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let start_type = self.infer_expression(start, context);

        if matches!(kind, RangeExpressionType::IterableObject) {
            return start_type;
        }

        if let Some(end_expr) = end {
            let end_type = self.infer_expression(end_expr, context);
            if !self.are_compatible(&start_type, &end_type, context) {
                self.report_error(
                    format!("Range types mismatch: {} and {}", start_type, end_type),
                    span,
                );
            }
        }

        let type_expr = self.create_type_expression(start_type);
        make_type(TypeKind::Custom("Range".to_string(), Some(vec![type_expr])))
    }

    fn infer_list(&mut self, elements: &[Expression], context: &mut Context) -> Type {
        if elements.is_empty() {
            return make_type(TypeKind::List(Box::new(
                self.create_type_expression(make_type(TypeKind::Void)),
            )));
        }

        let first_type = self.infer_expression(&elements[0], context);
        let mut has_error = false;

        for element in &elements[1..] {
            let element_type = self.infer_expression(element, context);
            if !self.are_compatible(&first_type, &element_type, context) {
                self.report_error(
                    "List elements must have the same type".to_string(),
                    element.span.clone(),
                );
                has_error = true;
            }
        }

        if has_error {
            return make_type(TypeKind::Error);
        }

        make_type(TypeKind::List(Box::new(
            self.create_type_expression(first_type),
        )))
    }

    fn infer_map(&mut self, entries: &[(Expression, Expression)], context: &mut Context) -> Type {
        if entries.is_empty() {
            return make_type(TypeKind::Map(
                Box::new(self.create_type_expression(make_type(TypeKind::Void))),
                Box::new(self.create_type_expression(make_type(TypeKind::Void))),
            ));
        }

        let (first_key, first_val) = &entries[0];
        let key_type = self.infer_expression(first_key, context);
        let val_type = self.infer_expression(first_val, context);
        let mut has_error = false;

        for (key, val) in &entries[1..] {
            let k_type = self.infer_expression(key, context);
            let v_type = self.infer_expression(val, context);

            if !self.are_compatible(&key_type, &k_type, context) {
                self.report_error(
                    "Map keys must have the same type".to_string(),
                    key.span.clone(),
                );
                has_error = true;
            }
            if !self.are_compatible(&val_type, &v_type, context) {
                self.report_error(
                    "Map values must have the same type".to_string(),
                    val.span.clone(),
                );
                has_error = true;
            }
        }

        if has_error {
            return make_type(TypeKind::Error);
        }

        make_type(TypeKind::Map(
            Box::new(self.create_type_expression(key_type)),
            Box::new(self.create_type_expression(val_type)),
        ))
    }

    fn infer_set(&mut self, elements: &[Expression], context: &mut Context) -> Type {
        if elements.is_empty() {
            return make_type(TypeKind::Set(Box::new(
                self.create_type_expression(make_type(TypeKind::Void)),
            )));
        }

        let first_type = self.infer_expression(&elements[0], context);
        let mut has_error = false;

        for element in &elements[1..] {
            let element_type = self.infer_expression(element, context);
            if !self.are_compatible(&first_type, &element_type, context) {
                self.report_error(
                    "Set elements must have the same type".to_string(),
                    element.span.clone(),
                );
                has_error = true;
            }
        }

        if has_error {
            return make_type(TypeKind::Error);
        }

        if let TypeKind::Nullable(_) = first_type.kind {
            self.report_error(
                "Set elements cannot be nullable".to_string(),
                elements[0].span.clone(),
            );
        }

        make_type(TypeKind::Set(Box::new(
            self.create_type_expression(first_type),
        )))
    }

    fn infer_tuple(&mut self, elements: &[Expression], context: &mut Context) -> Type {
        let mut element_types = Vec::new();
        for element in elements {
            let ty = self.infer_expression(element, context);
            element_types.push(self.create_type_expression(ty));
        }
        make_type(TypeKind::Tuple(element_types))
    }

    fn infer_index(
        &mut self,
        obj: &Expression,
        index: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let obj_type = self.infer_expression(obj, context);
        let index_type = self.infer_expression(index, context);

        // Check for Range index (Slicing)
        if let TypeKind::Custom(name, args) = &index_type.kind {
            if name == "Range" {
                // Ensure range is of integer type
                if let Some(args) = args {
                    if args.len() == 1 {
                        let range_inner = self.resolve_type_expression(&args[0], context);
                        if !matches!(range_inner.kind, TypeKind::Int) {
                            self.report_error(
                                "Slice range must be of integer type".to_string(),
                                index.span.clone(),
                            );
                            return make_type(TypeKind::Error);
                        }
                    }
                }

                match obj_type.kind {
                    TypeKind::String => return make_type(TypeKind::String),
                    TypeKind::List(inner) => return make_type(TypeKind::List(inner)),
                    TypeKind::Tuple(elements) => {
                        if elements.is_empty() {
                            return make_type(TypeKind::List(Box::new(
                                self.create_type_expression(make_type(TypeKind::Void)),
                            )));
                        }
                        let first = self.resolve_type_expression(&elements[0], context);
                        let is_homogeneous = elements.iter().all(|e| {
                            let t = self.resolve_type_expression(e, context);
                            self.are_compatible(&t, &first, context)
                        });

                        if is_homogeneous {
                            return make_type(TypeKind::List(Box::new(
                                self.create_type_expression(first),
                            )));
                        } else {
                            self.report_error("Cannot slice heterogeneous tuple".to_string(), span);
                            return make_type(TypeKind::Error);
                        }
                    }
                    _ => {
                        self.report_error(format!("Type {} is not sliceable", obj_type), span);
                        return make_type(TypeKind::Error);
                    }
                }
            }
        }

        match obj_type.kind {
            TypeKind::List(inner_type_expr) => {
                if !matches!(index_type.kind, TypeKind::Int) {
                    self.report_error(
                        "List index must be an integer".to_string(),
                        index.span.clone(),
                    );
                    return make_type(TypeKind::Error);
                }
                self.resolve_type_expression(&inner_type_expr, context)
            }
            TypeKind::Map(key_type_expr, val_type_expr) => {
                let key_type = self.resolve_type_expression(&key_type_expr, context);
                if !self.are_compatible(&key_type, &index_type, context) {
                    self.report_error("Invalid map key type".to_string(), index.span.clone());
                    return make_type(TypeKind::Error);
                }
                self.resolve_type_expression(&val_type_expr, context)
            }
            TypeKind::Tuple(element_type_exprs) => {
                // Check if tuple is homogeneous
                let is_homogeneous = if element_type_exprs.is_empty() {
                    true
                } else {
                    let resolved_types: Vec<Type> = element_type_exprs
                        .iter()
                        .map(|t| self.resolve_type_expression(t, context))
                        .collect();

                    let first_type = &resolved_types[0];
                    resolved_types
                        .iter()
                        .all(|t| self.are_compatible(t, first_type, context))
                };

                if is_homogeneous {
                    if !matches!(index_type.kind, TypeKind::Int) {
                        self.report_error(
                            "Tuple index must be an integer".to_string(),
                            index.span.clone(),
                        );
                        return make_type(TypeKind::Error);
                    }
                    // If homogeneous, we can return the type of the first element (or any element)
                    if element_type_exprs.is_empty() {
                        // Indexing empty tuple is always out of bounds, but let's handle it gracefully or error
                        self.report_error(
                            "Tuple index out of bounds (empty tuple)".to_string(),
                            span,
                        );
                        return make_type(TypeKind::Error);
                    }

                    // If it's a literal, we can still check bounds
                    if let ExpressionKind::Literal(Literal::Integer(val)) = &index.node {
                        let idx = match val {
                            crate::ast::IntegerLiteral::I8(v) => *v as usize,
                            crate::ast::IntegerLiteral::I16(v) => *v as usize,
                            crate::ast::IntegerLiteral::I32(v) => *v as usize,
                            crate::ast::IntegerLiteral::I64(v) => *v as usize,
                            crate::ast::IntegerLiteral::I128(v) => *v as usize,
                            crate::ast::IntegerLiteral::U8(v) => *v as usize,
                            crate::ast::IntegerLiteral::U16(v) => *v as usize,
                            crate::ast::IntegerLiteral::U32(v) => *v as usize,
                            crate::ast::IntegerLiteral::U64(v) => *v as usize,
                            crate::ast::IntegerLiteral::U128(v) => *v as usize,
                        };
                        if idx >= element_type_exprs.len() {
                            self.report_error("Tuple index out of bounds".to_string(), span);
                            return make_type(TypeKind::Error);
                        }
                    }

                    self.resolve_type_expression(&element_type_exprs[0], context)
                } else {
                    // For heterogeneous tuple, index must be a compile-time integer literal
                    if let ExpressionKind::Literal(Literal::Integer(val)) = &index.node {
                        let idx = match val {
                            crate::ast::IntegerLiteral::I8(v) => *v as usize,
                            crate::ast::IntegerLiteral::I16(v) => *v as usize,
                            crate::ast::IntegerLiteral::I32(v) => *v as usize,
                            crate::ast::IntegerLiteral::I64(v) => *v as usize,
                            crate::ast::IntegerLiteral::I128(v) => *v as usize,
                            crate::ast::IntegerLiteral::U8(v) => *v as usize,
                            crate::ast::IntegerLiteral::U16(v) => *v as usize,
                            crate::ast::IntegerLiteral::U32(v) => *v as usize,
                            crate::ast::IntegerLiteral::U64(v) => *v as usize,
                            crate::ast::IntegerLiteral::U128(v) => *v as usize,
                        };

                        if idx < element_type_exprs.len() {
                            self.resolve_type_expression(&element_type_exprs[idx], context)
                        } else {
                            self.report_error("Tuple index out of bounds".to_string(), span);
                            make_type(TypeKind::Error)
                        }
                    } else {
                        self.report_error(
                            "Tuple index must be an integer literal for heterogeneous tuples"
                                .to_string(),
                            index.span.clone(),
                        );
                        make_type(TypeKind::Error)
                    }
                }
            }
            TypeKind::String => {
                if !matches!(index_type.kind, TypeKind::Int) {
                    self.report_error(
                        "String index must be an integer".to_string(),
                        index.span.clone(),
                    );
                    return make_type(TypeKind::Error);
                }
                make_type(TypeKind::String) // Indexing a string returns a string (char)
            }
            TypeKind::Error => make_type(TypeKind::Error),
            _ => {
                self.report_error(format!("Type {} is not indexable", obj_type), span);
                make_type(TypeKind::Error)
            }
        }
    }

    fn infer_member(
        &mut self,
        obj: &Expression,
        prop: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let obj_type = self.infer_expression(obj, context);

        if let TypeKind::Tuple(element_types) = &obj_type.kind {
            if let ExpressionKind::Literal(Literal::Integer(val)) = &prop.node {
                let idx = match val {
                    crate::ast::IntegerLiteral::I8(v) => *v as usize,
                    crate::ast::IntegerLiteral::I16(v) => *v as usize,
                    crate::ast::IntegerLiteral::I32(v) => *v as usize,
                    crate::ast::IntegerLiteral::I64(v) => *v as usize,
                    crate::ast::IntegerLiteral::I128(v) => *v as usize,
                    crate::ast::IntegerLiteral::U8(v) => *v as usize,
                    crate::ast::IntegerLiteral::U16(v) => *v as usize,
                    crate::ast::IntegerLiteral::U32(v) => *v as usize,
                    crate::ast::IntegerLiteral::U64(v) => *v as usize,
                    crate::ast::IntegerLiteral::U128(v) => *v as usize,
                };

                if idx < element_types.len() {
                    return self.resolve_type_expression(&element_types[idx], context);
                } else {
                    self.report_error("Tuple index out of bounds".to_string(), span);
                    return make_type(TypeKind::Error);
                }
            }
        }

        let prop_name = if let ExpressionKind::Identifier(name, _) = &prop.node {
            name
        } else {
            self.report_error(
                "Member property must be an identifier".to_string(),
                prop.span.clone(),
            );
            return make_type(TypeKind::Error);
        };

        // Try to resolve the type definition for the object's type
        let (type_name, type_args) = match &obj_type.kind {
            TypeKind::String => (Some("String".to_string()), None),
            TypeKind::Custom(name, args) => (Some(name.clone()), args.clone()),
            TypeKind::Result(ok_type, _) => {
                if prop_name == "unwrap" {
                    let t = self.resolve_type_expression(ok_type, context);
                    return make_type(TypeKind::Function(
                        None,
                        vec![],
                        Some(Box::new(ast_factory::type_expr_non_null(t))),
                    ));
                } else if prop_name == "is_ok" || prop_name == "is_err" {
                    return make_type(TypeKind::Function(
                        None,
                        vec![],
                        Some(Box::new(ast_factory::type_expr_non_null(make_type(
                            TypeKind::Boolean,
                        )))),
                    ));
                }
                (None, None)
            }
            TypeKind::Nullable(inner_type) => {
                if prop_name == "unwrap" {
                    return make_type(TypeKind::Function(
                        None,
                        vec![],
                        Some(Box::new(ast_factory::type_expr_non_null(
                            *inner_type.clone(),
                        ))),
                    ));
                } else if prop_name == "is_some" || prop_name == "is_none" {
                    return make_type(TypeKind::Function(
                        None,
                        vec![],
                        Some(Box::new(ast_factory::type_expr_non_null(make_type(
                            TypeKind::Boolean,
                        )))),
                    ));
                }
                (None, None)
            }
            // For generic types with constraints (T extends SomeClass), use constraint for member lookup
            TypeKind::Generic(_, Some(constraint), _) => {
                // Use the constraint type for member lookup
                match &constraint.kind {
                    TypeKind::Custom(name, args) => (Some(name.clone()), args.clone()),
                    _ => (None, None),
                }
            }
            TypeKind::Generic(name, None, _) => {
                // Generic without constraint - no members
                self.report_error(
                    format!(
                        "Generic type '{}' without constraints has no known members",
                        name
                    ),
                    span,
                );
                return make_type(TypeKind::Error);
            }
            // Add others as needed
            _ => (None, None),
        };

        if let Some(name) = &type_name {
            if name == "Kernel" && prop_name == "launch" {
                // Method signature: fn(grid: Dim3, block: Dim3) -> Future<void>
                let dim3_type = ast_factory::make_type(TypeKind::Custom("Dim3".to_string(), None));
                let dim3_expr = Box::new(ast_factory::type_expr_non_null(dim3_type.clone()));

                let future_void_type = ast_factory::make_type(TypeKind::Custom(
                    "Future".to_string(),
                    Some(vec![ast_factory::type_expr_non_null(
                        ast_factory::make_type(TypeKind::Void),
                    )]),
                ));

                return ast_factory::make_type(TypeKind::Function(
                    None,
                    vec![
                        Parameter {
                            name: "grid".to_string(),
                            typ: dim3_expr.clone(),
                            guard: None,
                            default_value: None,
                        },
                        Parameter {
                            name: "block".to_string(),
                            typ: dim3_expr,
                            guard: None,
                            default_value: None,
                        },
                    ],
                    Some(Box::new(ast_factory::type_expr_non_null(future_void_type))),
                ));
            }
        }

        if let Some(name) = type_name {
            // Instance member access (Struct field)
            // We need to clone the definition to avoid borrowing issues with context
            let def_opt = context
                .resolve_type_definition(&name)
                .cloned()
                .or_else(|| self.global_type_definitions.get(&name).cloned());

            if let Some(TypeDefinition::Struct(def)) = def_opt {
                if let Some((_, field_type, visibility)) =
                    def.fields.iter().find(|(n, _, _)| n == prop_name)
                {
                    if !self.check_visibility(visibility, &def.module) {
                        self.report_error(format!("Field '{}' is not visible", prop_name), span);
                        return make_type(TypeKind::Error);
                    }

                    // Substitute generic parameters if present
                    if let Some(generics) = &def.generics {
                        if let Some(type_args) = &type_args {
                            if generics.len() == type_args.len() {
                                let mut mapping = HashMap::new();
                                for (param, arg_expr) in generics.iter().zip(type_args.iter()) {
                                    let arg_type = self
                                        .extract_type_from_expression(arg_expr)
                                        .unwrap_or(make_type(TypeKind::Error));
                                    mapping.insert(param.name.clone(), arg_type);
                                }
                                return self.substitute_type(field_type, &mapping);
                            }
                        }
                    }

                    return field_type.clone();
                } else {
                    let candidates: Vec<&str> =
                        def.fields.iter().map(|(n, _, _)| n.as_str()).collect();
                    if let Some(suggestion) = find_best_match(prop_name, &candidates) {
                        self.report_error_with_help(
                            format!("Type '{}' has no field '{}'", name, prop_name),
                            span,
                            format!("Did you mean '{}'?", suggestion),
                        );
                    } else {
                        self.report_error(
                            format!("Type '{}' has no field '{}'", name, prop_name),
                            span,
                        );
                    }
                    return make_type(TypeKind::Error);
                }
            } else if let Some(TypeDefinition::Class(def)) = def_opt {
                // Walk up the inheritance chain to find the member
                let mut search_class_def = def.clone();

                loop {
                    // Check fields in current class
                    if let Some(field_info) = search_class_def.fields.get(prop_name) {
                        // Check visibility for class field
                        if !self.check_member_visibility(
                            &field_info.visibility,
                            &search_class_def.name,
                            context.current_class.as_deref(),
                        ) {
                            self.report_error(
                                format!(
                                    "Field '{}' of class '{}' is {:?} and cannot be accessed from here",
                                    prop_name, search_class_def.name, field_info.visibility
                                ),
                                span,
                            );
                            return make_type(TypeKind::Error);
                        }
                        return field_info.ty.clone();
                    }

                    // Check methods in current class
                    if let Some(method_info) = search_class_def.methods.get(prop_name) {
                        // Check visibility for class method
                        if !self.check_member_visibility(
                            &method_info.visibility,
                            &search_class_def.name,
                            context.current_class.as_deref(),
                        ) {
                            self.report_error(
                                format!(
                                    "Method '{}' of class '{}' is {:?} and cannot be accessed from here",
                                    prop_name, search_class_def.name, method_info.visibility
                                ),
                                span,
                            );
                            return make_type(TypeKind::Error);
                        }

                        // Build a function type from the method signature
                        let params: Vec<Parameter> = method_info
                            .params
                            .iter()
                            .map(|(name, ty)| Parameter {
                                name: name.clone(),
                                typ: Box::new(self.create_type_expression(ty.clone())),
                                guard: None,
                                default_value: None,
                            })
                            .collect();

                        let return_type_expr =
                            if matches!(method_info.return_type.kind, TypeKind::Void) {
                                None
                            } else {
                                Some(Box::new(
                                    self.create_type_expression(method_info.return_type.clone()),
                                ))
                            };

                        return make_type(TypeKind::Function(None, params, return_type_expr));
                    }

                    // If not found, try the base class
                    if let Some(base_class_name) = &search_class_def.base_class {
                        let base_def_opt = context
                            .resolve_type_definition(base_class_name)
                            .cloned()
                            .or_else(|| self.global_type_definitions.get(base_class_name).cloned());

                        if let Some(TypeDefinition::Class(base_def)) = base_def_opt {
                            search_class_def = base_def;
                            continue;
                        }
                    }

                    // No more base classes, member not found
                    break;
                }

                // Collect all candidates from the class hierarchy for suggestions
                let mut candidates: Vec<String> = Vec::new();
                let mut collect_class_name = name.clone();
                loop {
                    let collect_def_opt = context
                        .resolve_type_definition(&collect_class_name)
                        .cloned()
                        .or_else(|| {
                            self.global_type_definitions
                                .get(&collect_class_name)
                                .cloned()
                        });

                    if let Some(TypeDefinition::Class(collect_def)) = collect_def_opt {
                        candidates.extend(collect_def.fields.keys().cloned());
                        candidates.extend(collect_def.methods.keys().cloned());

                        if let Some(base_name) = &collect_def.base_class {
                            collect_class_name = base_name.clone();
                            continue;
                        }
                    }
                    break;
                }

                let candidate_refs: Vec<&str> = candidates.iter().map(|s| s.as_str()).collect();
                if let Some(suggestion) = find_best_match(prop_name, &candidate_refs) {
                    self.report_error_with_help(
                        format!("Type '{}' has no field or method '{}'", name, prop_name),
                        span,
                        format!("Did you mean '{}'?", suggestion),
                    );
                } else {
                    self.report_error(
                        format!("Type '{}' has no field or method '{}'", name, prop_name),
                        span,
                    );
                }
                return make_type(TypeKind::Error);
            } else if let Some(TypeDefinition::Enum(_)) = def_opt {
                // Could be an enum instance, but enums don't have fields yet (unless methods are added later)
                self.report_error(format!("Type '{}' does not have members", name), span);
                return make_type(TypeKind::Error);
            }
        }

        match obj_type.kind {
            TypeKind::Meta(inner_type) => {
                // Static member access (Enum variant)
                if let TypeKind::Custom(name, _) = &inner_type.kind {
                    let def_opt = context
                        .resolve_type_definition(name)
                        .cloned()
                        .or_else(|| self.global_type_definitions.get(name).cloned());

                    if let Some(TypeDefinition::Enum(def)) = def_opt {
                        if let Some(variant_types) = def.variants.get(prop_name) {
                            // If variant has no associated types, it's a value of the Enum type.
                            // If it has associated types, it's a constructor function.

                            // Check for generics substitution
                            let type_args = if let TypeKind::Custom(_, args) = &inner_type.kind {
                                args.clone()
                            } else {
                                None
                            };

                            if variant_types.is_empty() {
                                make_type(TypeKind::Custom(name.clone(), type_args))
                            } else {
                                // Constructor function: (args) -> EnumType

                                // Perform substitution if needed
                                let mut substituted_variant_types = Vec::new();
                                if let Some(generics) = &def.generics {
                                    if let Some(args) = &type_args {
                                        if generics.len() == args.len() {
                                            let mut mapping = HashMap::new();
                                            for (param, arg_expr) in
                                                generics.iter().zip(args.iter())
                                            {
                                                let arg_type = self
                                                    .extract_type_from_expression(arg_expr)
                                                    .unwrap_or(make_type(TypeKind::Error));
                                                mapping.insert(param.name.clone(), arg_type);
                                            }

                                            for t in variant_types {
                                                substituted_variant_types
                                                    .push(self.substitute_type(t, &mapping));
                                            }
                                        } else {
                                            substituted_variant_types = variant_types.clone();
                                        }
                                    } else {
                                        substituted_variant_types = variant_types.clone();
                                    }
                                } else {
                                    substituted_variant_types = variant_types.clone();
                                }

                                let params: Vec<Parameter> = substituted_variant_types
                                    .iter()
                                    .enumerate()
                                    .map(|(i, t)| Parameter {
                                        name: format!("arg{}", i),
                                        typ: Box::new(self.create_type_expression(t.clone())),
                                        guard: None,
                                        default_value: None,
                                    })
                                    .collect();
                                make_type(TypeKind::Function(
                                    None,
                                    params,
                                    Some(Box::new(self.create_type_expression(make_type(
                                        TypeKind::Custom(name.clone(), type_args),
                                    )))),
                                ))
                            }
                        } else {
                            let candidates: Vec<&str> =
                                def.variants.keys().map(|s| s.as_str()).collect();
                            if let Some(suggestion) = find_best_match(prop_name, &candidates) {
                                self.report_error_with_help(
                                    format!("Enum '{}' has no variant '{}'", name, prop_name),
                                    span,
                                    format!("Did you mean '{}'?", suggestion),
                                );
                            } else {
                                self.report_error(
                                    format!("Enum '{}' has no variant '{}'", name, prop_name),
                                    span,
                                );
                            }
                            make_type(TypeKind::Error)
                        }
                    } else {
                        self.report_error(
                            format!("Type '{}' does not have static members", name),
                            span,
                        );
                        make_type(TypeKind::Error)
                    }
                } else {
                    self.report_error(
                        format!("Type '{}' does not have static members", inner_type),
                        span,
                    );
                    make_type(TypeKind::Error)
                }
            }
            TypeKind::String => match prop_name.as_str() {
                "length" => make_type(TypeKind::Int),
                _ => {
                    self.report_error(format!("Type 'String' has no field '{}'", prop_name), span);
                    make_type(TypeKind::Error)
                }
            },
            _ => {
                self.report_error(format!("Type '{}' does not have members", obj_type), span);
                make_type(TypeKind::Error)
            }
        }
    }

    fn infer_match(
        &mut self,
        subject: &Expression,
        branches: &[MatchBranch],
        span: Span,
        context: &mut Context,
    ) -> Type {
        let subject_type = self.infer_expression(subject, context);

        // Check exhaustiveness for Enums
        if let TypeKind::Custom(name, _) = &subject_type.kind {
            // Find enum definition
            let mut enum_def_opt = None;

            // Check local scopes first (reverse order)
            for scope in context.type_definitions.iter().rev() {
                if let Some(TypeDefinition::Enum(def)) = scope.get(name) {
                    enum_def_opt = Some(def);
                    break;
                }
            }

            // Check global scope if not found locally
            if enum_def_opt.is_none() {
                if let Some(TypeDefinition::Enum(def)) = self.global_type_definitions.get(name) {
                    enum_def_opt = Some(def);
                }
            }

            if let Some(enum_def) = enum_def_opt {
                let mut remaining_variants: HashSet<String> =
                    enum_def.variants.keys().cloned().collect();
                let mut is_exhaustive = false;

                for branch in branches {
                    // Only unguarded patterns count toward exhaustiveness
                    if branch.guard.is_none() {
                        for pattern in &branch.patterns {
                            match pattern {
                                Pattern::Default => {
                                    is_exhaustive = true;
                                }
                                Pattern::Identifier(_) => {
                                    // Variable binding covers everything
                                    is_exhaustive = true;
                                }
                                Pattern::Member(parent, member) => {
                                    // Check if parent is the enum name
                                    if let Pattern::Identifier(parent_name) = &**parent {
                                        if parent_name == name {
                                            remaining_variants.remove(member);
                                        }
                                    }
                                }
                                Pattern::EnumVariant(parent, _) => {
                                    if let Pattern::Member(enum_name_pat, variant_name) = &**parent
                                    {
                                        if let Pattern::Identifier(enum_name_str) = &**enum_name_pat
                                        {
                                            if enum_name_str == name {
                                                remaining_variants.remove(variant_name);
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if is_exhaustive {
                        break;
                    }
                }

                if !is_exhaustive && !remaining_variants.is_empty() {
                    let mut missing: Vec<_> = remaining_variants.into_iter().collect();
                    missing.sort();
                    self.report_error(
                        format!(
                            "Non-exhaustive match on Enum '{}'. Missing variants: {}",
                            name,
                            missing.join(", ")
                        ),
                        span.clone(),
                    );
                }
            }
        }

        if branches.is_empty() {
            return make_type(TypeKind::Void);
        }

        let mut first_branch_type = None;

        for branch in branches {
            context.enter_scope();
            for pattern in &branch.patterns {
                self.check_pattern(pattern, &subject_type, context, span.clone());
            }

            let body_type = self.infer_statement_type(&branch.body, context);
            context.exit_scope();

            if let Some(first) = &first_branch_type {
                if !self.are_compatible(first, &body_type, context) {
                    self.report_error(
                        format!(
                            "Match branch types mismatch: expected {}, got {}",
                            first, body_type
                        ),
                        span.clone(),
                    );
                }
            } else {
                first_branch_type = Some(body_type);
            }
        }

        first_branch_type.unwrap_or(make_type(TypeKind::Void))
    }

    fn infer_lambda(
        &mut self,
        generics: &Option<Vec<Expression>>,
        params: &[Parameter],
        return_type_expr: &Option<Box<Expression>>,
        body: &Statement,
        _properties: &FunctionProperties,
        context: &mut Context,
    ) -> Type {
        context.enter_scope();

        if let Some(gens) = generics {
            self.define_generics(gens, context);
        }

        // Determine expected return type
        let expected_return_type = return_type_expr
            .as_ref()
            .map(|rt_expr| self.resolve_type_expression(rt_expr, context));

        if let Some(rt) = &expected_return_type {
            context.return_types.push(rt.clone());
            context.inferred_return_types.push(None);
        } else {
            context.return_types.push(make_type(TypeKind::Void)); // Placeholder
            context.inferred_return_types.push(Some(Vec::new()));
        }

        // Reset loop depth for function body as it's a new context
        let old_loop_depth = context.loop_depth;
        context.loop_depth = 0;

        for param in params {
            let param_type = self.resolve_type_expression(&param.typ, context);
            context.define(
                param.name.clone(),
                param_type,
                false,
                false,
                MemberVisibility::Public,
                self.current_module.clone(),
                None,
            ); // Parameters are immutable by default
        }

        // Check body and infer implicit return type
        let implicit_return_type = match &body.node {
            StatementKind::Block(stmts) => {
                context.enter_scope();
                let mut last_type = make_type(TypeKind::Void);
                for (i, stmt) in stmts.iter().enumerate() {
                    if i == stmts.len() - 1 {
                        if let StatementKind::Expression(expr) = &stmt.node {
                            last_type = self.infer_expression(expr, context);
                        } else {
                            self.check_statement(stmt, context);
                        }
                    } else {
                        self.check_statement(stmt, context);
                    }
                }
                context.exit_scope();
                last_type
            }
            StatementKind::Expression(expr) => self.infer_expression(expr, context),
            _ => {
                self.check_statement(body, context);
                make_type(TypeKind::Void)
            }
        };

        // Finalize return type
        let final_return_type_expr = if let Some(expected) = expected_return_type {
            let is_void_implicit = matches!(implicit_return_type.kind, TypeKind::Void);
            let is_void_expected = matches!(expected.kind, TypeKind::Void);

            if !is_void_expected && is_void_implicit {
                // Check if the last statement was a return statement?
                let ends_with_return = match &body.node {
                    StatementKind::Block(stmts) => {
                        if let Some(last) = stmts.last() {
                            matches!(last.node, StatementKind::Return(_))
                        } else {
                            false
                        }
                    }
                    StatementKind::Return(_) => true,
                    _ => false,
                };

                if !ends_with_return {
                    self.report_error(
                        format!(
                            "Invalid return type: expected {}, got {}",
                            expected, implicit_return_type
                        ),
                        body.span.clone(),
                    );
                }
            } else if !self.are_compatible(&expected, &implicit_return_type, context)
                && !matches!(expected.kind, TypeKind::Void)
            {
                self.report_error(
                    format!(
                        "Invalid return type: expected {}, got {}",
                        expected, implicit_return_type
                    ),
                    body.span.clone(),
                );
            }
            return_type_expr.clone()
        } else {
            // Inference
            let collected_returns = context
                .inferred_return_types
                .pop()
                .unwrap_or_else(|| {
                    // Should not happen if stack is balanced
                    Some(Vec::new())
                })
                .unwrap_or_default();
            context.return_types.pop(); // Pop the placeholder

            let mut candidate = implicit_return_type;

            for (ret_ty, ret_span) in collected_returns {
                if matches!(candidate.kind, TypeKind::Void) {
                    candidate = ret_ty;
                } else if !matches!(ret_ty.kind, TypeKind::Void) {
                    if !self.are_compatible(&candidate, &ret_ty, context) {
                        self.report_error(
                            format!(
                                "Incompatible return types in lambda: {} and {}",
                                candidate, ret_ty
                            ),
                            ret_span,
                        );
                    }
                } else {
                    // candidate is not Void, ret_ty is Void.
                    self.report_error(
                        format!(
                            "Incompatible return types in lambda: {} and {}",
                            candidate, ret_ty
                        ),
                        ret_span,
                    );
                }
            }

            Some(Box::new(self.create_type_expression(candidate)))
        };

        if return_type_expr.is_some() {
            context.return_types.pop();
            context.inferred_return_types.pop();
        }

        context.loop_depth = old_loop_depth;
        context.exit_scope();

        make_type(TypeKind::Function(
            generics.clone(),
            params.to_vec(),
            final_return_type_expr,
        ))
    }

    fn infer_conditional(
        &mut self,
        then_expr: &Expression,
        cond_expr: &Expression,
        else_expr_opt: &Option<Box<Expression>>,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let cond_type = self.infer_expression(cond_expr, context);
        if !matches!(cond_type.kind, TypeKind::Boolean) {
            self.report_error(
                format!("Conditional condition must be a boolean, got {}", cond_type),
                cond_expr.span.clone(),
            );
        }

        let then_type = self.infer_expression(then_expr, context);

        if let Some(else_expr) = else_expr_opt {
            let else_type = self.infer_expression(else_expr, context);
            if !self.are_compatible(&then_type, &else_type, context) {
                self.report_error(
                    format!(
                        "Conditional branches must have the same type: expected {}, got {}",
                        then_type, else_type
                    ),
                    span,
                );
            }
            then_type
        } else {
            if !self.are_compatible(&then_type, &make_type(TypeKind::Void), context) {
                self.report_error(
                    format!(
                        "Conditional expression without else branch must return Void, got {}",
                        then_type
                    ),
                    span,
                );
            }
            make_type(TypeKind::Void)
        }
    }

    fn infer_formatted_string(&mut self, parts: &[Expression], context: &mut Context) -> Type {
        for part in parts {
            self.infer_expression(part, context);
        }
        make_type(TypeKind::String)
    }

    fn check_pattern(
        &mut self,
        pattern: &Pattern,
        subject_type: &Type,
        context: &mut Context,
        span: Span,
    ) {
        match pattern {
            Pattern::Literal(lit) => {
                let lit_type = self.infer_literal(lit);
                if !self.are_compatible(subject_type, &lit_type, context) {
                    self.report_error(
                        format!(
                            "Pattern type mismatch: expected {}, got {}",
                            subject_type, lit_type
                        ),
                        span,
                    );
                }
            }
            Pattern::Identifier(name) => {
                // Bind variable
                context.define(
                    name.clone(),
                    subject_type.clone(),
                    false,
                    false,
                    MemberVisibility::Public,
                    self.current_module.clone(),
                    None,
                ); // Immutable binding by default
            }
            Pattern::Tuple(patterns) => {
                if let TypeKind::Tuple(elem_types) = &subject_type.kind {
                    if patterns.len() != elem_types.len() {
                        self.report_error(
                            format!(
                                "Tuple pattern length mismatch: expected {}, got {}",
                                elem_types.len(),
                                patterns.len()
                            ),
                            span.clone(),
                        );
                        return;
                    }

                    // Clone to avoid borrowing issues
                    let elem_types_cloned = elem_types.clone();

                    for (i, pat) in patterns.iter().enumerate() {
                        let elem_type =
                            self.resolve_type_expression(&elem_types_cloned[i], context);
                        self.check_pattern(pat, &elem_type, context, span.clone());
                    }
                } else {
                    self.report_error(
                        format!(
                            "Expected tuple type for tuple pattern, got {}",
                            subject_type
                        ),
                        span,
                    );
                }
            }
            Pattern::Member(parent, member) => {
                if let Pattern::Identifier(parent_name) = &**parent {
                    let enum_def_opt = context
                        .resolve_type_definition(parent_name)
                        .cloned()
                        .or_else(|| self.global_type_definitions.get(parent_name).cloned());
                    if let Some(TypeDefinition::Enum(enum_def)) = enum_def_opt {
                        if !enum_def.variants.contains_key(member) {
                            self.report_error(
                                format!("Enum '{}' has no variant '{}'", parent_name, member),
                                span.clone(),
                            );
                        }
                        // Check if subject type matches the enum type
                        // We construct the expected type from the enum name, preserving generic args if present in subject
                        let expected_type = if let TypeKind::Custom(sub_name, sub_args) =
                            &subject_type.kind
                        {
                            if sub_name == parent_name {
                                make_type(TypeKind::Custom(parent_name.clone(), sub_args.clone()))
                            } else {
                                make_type(TypeKind::Custom(parent_name.clone(), None))
                            }
                        } else {
                            make_type(TypeKind::Custom(parent_name.clone(), None))
                        };
                        if !self.are_compatible(subject_type, &expected_type, context) {
                            self.report_error(
                                format!(
                                    "Pattern type mismatch: expected {}, got {}",
                                    subject_type, expected_type
                                ),
                                span,
                            );
                        }
                    } else {
                        self.report_error(format!("'{}' is not an Enum", parent_name), span);
                    }
                } else {
                    self.report_error(
                        "Complex member patterns are not supported".to_string(),
                        span,
                    );
                }
            }
            Pattern::Regex(_) => {
                if !matches!(subject_type.kind, TypeKind::String) {
                    self.report_error(
                        format!(
                            "Regex pattern requires string subject, got {}",
                            subject_type
                        ),
                        span,
                    );
                }
            }
            Pattern::Default => {}
            Pattern::EnumVariant(parent_pattern, bindings) => {
                // Extract enum name and variant name from parent pattern
                let (enum_name, variant_name) = match &**parent_pattern {
                    Pattern::Member(enum_pat, variant) => {
                        if let Pattern::Identifier(name) = &**enum_pat {
                            (name.clone(), variant.clone())
                        } else {
                            self.report_error(
                                "Complex member patterns are not supported".to_string(),
                                span.clone(),
                            );
                            return;
                        }
                    }
                    Pattern::Identifier(name) => {
                        // Could be just a variant if subject type is known enum
                        self.report_error(
                            format!("Expected enum variant pattern like EnumName.{}", name),
                            span,
                        );
                        return;
                    }
                    _ => {
                        self.report_error("Invalid enum variant pattern".to_string(), span);
                        return;
                    }
                };

                // Look up enum definition
                let enum_def_opt = context
                    .resolve_type_definition(&enum_name)
                    .cloned()
                    .or_else(|| self.global_type_definitions.get(&enum_name).cloned());
                if let Some(TypeDefinition::Enum(enum_def)) = enum_def_opt {
                    if let Some(variant_types) = enum_def.variants.get(&variant_name) {
                        // Check binding count matches
                        if bindings.len() != variant_types.len() {
                            self.report_error(
                                format!(
                                    "Enum variant '{}' expects {} bindings, got {}",
                                    variant_name,
                                    variant_types.len(),
                                    bindings.len()
                                ),
                                span.clone(),
                            );
                            return;
                        }

                        // Clone to avoid borrowing issues
                        let variant_types_cloned = variant_types.clone();

                        // Build generic mapping from subject_type's generic args
                        let generic_mapping: HashMap<String, Type> =
                            if let TypeKind::Custom(_, Some(ref args)) = &subject_type.kind {
                                if let Some(ref generics) = enum_def.generics {
                                    generics
                                        .iter()
                                        .zip(args.iter())
                                        .filter_map(|(g, arg_expr)| {
                                            self.extract_type_from_expression(arg_expr)
                                                .ok()
                                                .map(|ty| (g.name.clone(), ty))
                                        })
                                        .collect()
                                } else {
                                    HashMap::new()
                                }
                            } else {
                                HashMap::new()
                            };

                        // Bind each pattern with its type (substituting generics if needed)
                        for (binding, var_type) in bindings.iter().zip(variant_types_cloned.iter())
                        {
                            let resolved_type = if generic_mapping.is_empty() {
                                var_type.clone()
                            } else {
                                self.substitute_type(var_type, &generic_mapping)
                            };
                            self.check_pattern(binding, &resolved_type, context, span.clone());
                        }

                        // Check if subject type matches the enum type
                        // Preserve generic args from subject_type
                        let generic_args =
                            if let TypeKind::Custom(sub_name, ref sub_args) = &subject_type.kind {
                                if sub_name == &enum_name {
                                    sub_args.clone()
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                        let expected_type =
                            make_type(TypeKind::Custom(enum_name.clone(), generic_args));
                        if !self.are_compatible(subject_type, &expected_type, context) {
                            self.report_error(
                                format!(
                                    "Pattern type mismatch: expected {}, got {}",
                                    subject_type, expected_type
                                ),
                                span,
                            );
                        }
                    } else {
                        self.report_error(
                            format!("Enum '{}' has no variant '{}'", enum_name, variant_name),
                            span,
                        );
                    }
                } else {
                    self.report_error(format!("'{}' is not an Enum", enum_name), span);
                }
            }
        }
    }

    fn infer_statement_type(&mut self, stmt: &Statement, context: &mut Context) -> Type {
        match &stmt.node {
            StatementKind::Expression(expr) => self.infer_expression(expr, context),
            StatementKind::Block(stmts) => {
                context.enter_scope();
                let mut last_type = make_type(TypeKind::Void);
                for (i, s) in stmts.iter().enumerate() {
                    if i == stmts.len() - 1 {
                        last_type = self.infer_statement_type(s, context);
                    } else {
                        self.check_statement(s, context);
                    }
                }
                context.exit_scope();
                last_type
            }
            _ => {
                self.check_statement(stmt, context);
                make_type(TypeKind::Void)
            }
        }
    }

    fn infer_generic_instantiation(
        &mut self,
        expr: &Expression,
        generics: &Option<Vec<Expression>>,
        kind: &TypeDeclarationKind,
        target: &Option<Box<Expression>>,
        span: Span,
        context: &mut Context,
    ) -> Type {
        if *kind == TypeDeclarationKind::None && target.is_none() {
            if let Some(args) = generics {
                let expr_type = self.infer_expression(expr, context);
                match expr_type.kind {
                    TypeKind::Function(Some(params), func_params, ret) => {
                        let mut mapping = HashMap::new();
                        if params.len() != args.len() {
                            self.report_error("Generic argument count mismatch".to_string(), span);
                            return make_type(TypeKind::Error);
                        }

                        for (i, param) in params.iter().enumerate() {
                            if let ExpressionKind::GenericType(name_expr, _, _) = &param.node {
                                if let ExpressionKind::Identifier(name, _) = &name_expr.node {
                                    let arg_type = self.resolve_type_expression(&args[i], context);
                                    mapping.insert(name.clone(), arg_type);
                                }
                            }
                        }

                        let mut new_params = Vec::new();
                        for p in func_params {
                            let p_type = self
                                .extract_type_from_expression(&p.typ)
                                .unwrap_or(make_type(TypeKind::Error));
                            let new_p_type = self.substitute_type(&p_type, &mapping);
                            new_params.push(Parameter {
                                name: p.name.clone(),
                                typ: Box::new(self.create_type_expression(new_p_type)),
                                guard: p.guard.clone(),
                                default_value: p.default_value.clone(),
                            });
                        }

                        let new_ret = if let Some(r) = ret {
                            let r_type = self
                                .extract_type_from_expression(&r)
                                .unwrap_or(make_type(TypeKind::Error));
                            let new_r_type = self.substitute_type(&r_type, &mapping);
                            Some(Box::new(self.create_type_expression(new_r_type)))
                        } else {
                            None
                        };

                        return make_type(TypeKind::Function(None, new_params, new_ret));
                    }
                    TypeKind::Meta(inner) => {
                        if let TypeKind::Custom(name, _) = inner.kind {
                            let resolved_args: Vec<Expression> = args
                                .iter()
                                .map(|arg| {
                                    let ty = self.resolve_type_expression(arg, context);
                                    self.create_type_expression(ty)
                                })
                                .collect();
                            return make_type(TypeKind::Meta(Box::new(make_type(
                                TypeKind::Custom(name, Some(resolved_args)),
                            ))));
                        } else {
                            self.report_error("Expected generic type".to_string(), span);
                            return make_type(TypeKind::Error);
                        }
                    }
                    _ => {
                        self.report_error("Expected generic function or type".to_string(), span);
                        return make_type(TypeKind::Error);
                    }
                }
            }
        }
        make_type(TypeKind::Error)
    }
}
