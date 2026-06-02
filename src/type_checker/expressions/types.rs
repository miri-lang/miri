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

use crate::ast::factory as ast_factory;
use crate::ast::factory::make_type;
use crate::ast::types::{Type, TypeDeclarationKind, TypeKind};
use crate::ast::*;
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, TypeDefinition};
use crate::type_checker::TypeChecker;
use std::collections::HashMap;

impl TypeChecker {
    pub(crate) fn infer_enum_value(
        &mut self,
        name: &Expression,
        values: &[Expression],
        span: Span,
        context: &mut Context,
    ) -> Type {
        if let ExpressionKind::Identifier(id_name, _) = &name.node {
            if let Some(result_ty) = self.try_builtin_result_variant(id_name, values, span, context)
            {
                return result_ty;
            }
        }

        if let ExpressionKind::Member(enum_name_expr, variant_name_expr) = &name.node {
            if let Some(result_ty) =
                self.try_user_enum_variant(enum_name_expr, variant_name_expr, values, span, context)
            {
                return result_ty;
            }
        }

        ast_factory::make_type(TypeKind::Error)
    }

    fn try_builtin_result_variant(
        &mut self,
        id_name: &str,
        values: &[Expression],
        span: Span,
        context: &mut Context,
    ) -> Option<Type> {
        match id_name {
            "Ok" => {
                if values.len() != 1 {
                    self.report_error("Ok expects exactly 1 argument".to_string(), span);
                    return Some(ast_factory::make_type(TypeKind::Error));
                }
                let val_type = self.infer_expression(&values[0], context);
                Some(ast_factory::make_type(TypeKind::Custom(
                    "Result".to_string(),
                    Some(vec![
                        ast_factory::type_expr_non_null(val_type),
                        ast_factory::type_expr_non_null(ast_factory::make_type(TypeKind::Void)),
                    ]),
                )))
            }
            "Err" => {
                if values.len() != 1 {
                    self.report_error("Err expects exactly 1 argument".to_string(), span);
                    return Some(ast_factory::make_type(TypeKind::Error));
                }
                let val_type = self.infer_expression(&values[0], context);
                Some(ast_factory::make_type(TypeKind::Custom(
                    "Result".to_string(),
                    Some(vec![
                        ast_factory::type_expr_non_null(ast_factory::make_type(TypeKind::Void)),
                        ast_factory::type_expr_non_null(val_type),
                    ]),
                )))
            }
            _ => None,
        }
    }

    fn try_user_enum_variant(
        &mut self,
        enum_name_expr: &Expression,
        variant_name_expr: &Expression,
        values: &[Expression],
        span: Span,
        context: &mut Context,
    ) -> Option<Type> {
        if let (
            ExpressionKind::Identifier(enum_name, _),
            ExpressionKind::Identifier(variant_name, _),
        ) = (&enum_name_expr.node, &variant_name_expr.node)
        {
            let enum_def_opt = self.resolve_visible_type(enum_name, context).cloned();
            if let Some(TypeDefinition::Enum(enum_def)) = enum_def_opt {
                return self.validate_and_construct_enum_variant(
                    enum_name,
                    variant_name,
                    &enum_def,
                    values,
                    span,
                    context,
                );
            } else {
                self.report_error(format!("'{}' is not an Enum", enum_name), span);
                return Some(ast_factory::make_type(TypeKind::Error));
            }
        }
        None
    }

    fn validate_and_construct_enum_variant(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        enum_def: &crate::type_checker::context::EnumDefinition,
        values: &[Expression],
        span: Span,
        context: &mut Context,
    ) -> Option<Type> {
        let Some(variant_types) = enum_def.variants.get(variant_name) else {
            self.report_error(
                format!("Enum '{}' has no variant '{}'", enum_name, variant_name),
                span,
            );
            return Some(ast_factory::make_type(TypeKind::Error));
        };

        if values.len() != variant_types.len() {
            self.report_error(
                format!(
                    "Enum variant '{}.{}' expects {} arguments, got {}",
                    enum_name,
                    variant_name,
                    variant_types.len(),
                    values.len()
                ),
                span,
            );
            return Some(ast_factory::make_type(TypeKind::Error));
        }

        let generic_mapping = self.build_enum_generic_mapping(
            enum_def,
            variant_types,
            values,
            enum_name,
            variant_name,
            span,
            context,
        );

        if enum_def.generics.is_some() && !generic_mapping.is_empty() {
            self.validate_generic_enum_args(
                enum_name,
                variant_name,
                variant_types,
                values,
                &generic_mapping,
                context,
            );
        }

        let generic_args = self.build_enum_variant_generic_args(enum_def, &generic_mapping);

        Some(ast_factory::make_type(TypeKind::Custom(
            enum_name.to_string(),
            generic_args,
        )))
    }

    /// Builds the generic type arguments for an enum variant.
    fn build_enum_variant_generic_args(
        &mut self,
        enum_def: &crate::type_checker::context::EnumDefinition,
        generic_mapping: &HashMap<String, Type>,
    ) -> Option<Vec<Expression>> {
        enum_def.generics.as_ref().map(|generics| {
            generics
                .iter()
                .map(|g| {
                    let ty = generic_mapping
                        .get(&g.name)
                        .cloned()
                        .unwrap_or_else(|| ast_factory::make_type(TypeKind::Error));
                    self.create_type_expression(ty)
                })
                .collect()
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn build_enum_generic_mapping(
        &mut self,
        enum_def: &crate::type_checker::context::EnumDefinition,
        variant_types: &[Type],
        values: &[Expression],
        enum_name: &str,
        variant_name: &str,
        _span: Span,
        context: &mut Context,
    ) -> HashMap<String, Type> {
        if let Some(ref generics) = enum_def.generics {
            let mut mapping = HashMap::new();
            for (val, var_type) in values.iter().zip(variant_types.iter()) {
                let val_type = self.infer_expression(val, context);
                if let TypeKind::Generic(name, _, _) = &var_type.kind {
                    mapping.insert(name.clone(), val_type);
                }
            }
            for g in generics {
                mapping.entry(g.name.clone()).or_insert_with(|| {
                    ast_factory::make_type(TypeKind::Generic(
                        g.name.clone(),
                        None,
                        crate::ast::types::TypeDeclarationKind::None,
                    ))
                });
            }
            mapping
        } else {
            for (val, var_type) in values.iter().zip(variant_types.iter()) {
                let val_type = self.infer_expression(val, context);
                if !self.are_compatible(&val_type, var_type, context) {
                    self.report_error(
                        format!(
                            "Type mismatch in enum variant '{}.{}': expected {}, got {}",
                            enum_name, variant_name, var_type, val_type
                        ),
                        val.span,
                    );
                }
            }
            HashMap::new()
        }
    }

    fn validate_generic_enum_args(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        variant_types: &[Type],
        values: &[Expression],
        generic_mapping: &HashMap<String, Type>,
        context: &mut Context,
    ) {
        for (val, var_type) in values.iter().zip(variant_types.iter()) {
            let val_type = self.infer_expression(val, context);
            let substituted = self.substitute_type(var_type, generic_mapping);
            if !self.are_compatible(&val_type, &substituted, context) {
                self.report_error(
                    format!(
                        "Type mismatch in enum variant '{}.{}': expected {}, got {}",
                        enum_name, variant_name, substituted, val_type
                    ),
                    val.span,
                );
            }
        }
    }

    pub(crate) fn infer_generic_instantiation(
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
                    TypeKind::Function(func_data) if func_data.generics.is_some() => {
                        return self.instantiate_generic_function(&func_data, args, span, context);
                    }
                    TypeKind::Meta(inner) => {
                        return self.instantiate_generic_type(&inner, args, span, context);
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

    /// Instantiate generic parameters for a generic function
    fn instantiate_generic_function(
        &mut self,
        func_data: &crate::ast::types::FunctionTypeData,
        args: &[Expression],
        span: Span,
        context: &mut Context,
    ) -> Type {
        let Some(params) = func_data.generics.as_ref() else {
            return make_type(TypeKind::Error);
        };
        let func_params = &func_data.params;
        let ret = &func_data.return_type;
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

        let mut new_params = Vec::with_capacity(func_params.len());
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
                is_out: p.is_out,
            });
        }

        let new_ret = if let Some(r) = ret {
            let r_type = self
                .extract_type_from_expression(r)
                .unwrap_or(make_type(TypeKind::Error));
            let new_r_type = self.substitute_type(&r_type, &mapping);
            Some(Box::new(self.create_type_expression(new_r_type)))
        } else {
            None
        };

        make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: None,
            params: new_params,
            return_type: new_ret,
        })))
    }

    /// Instantiate generic arguments for a custom type (Meta type)
    fn instantiate_generic_type(
        &mut self,
        inner: &Type,
        args: &[Expression],
        span: Span,
        context: &mut Context,
    ) -> Type {
        if let TypeKind::Custom(name, _) = &inner.kind {
            let resolved_args: Vec<Expression> = args
                .iter()
                .map(|arg| {
                    // Type generics resolve through the normal `Type → Type`
                    // pipeline; value generics (a literal `3` in
                    // `Foo<float, 3>`) carry through verbatim. Routing the
                    // literal through `resolve_type_expression` would report
                    // "Expected type expression" and collapse the slot to
                    // `Error`, which then propagates into every field
                    // substitution downstream.
                    if self.extract_type_from_expression(arg).is_ok() {
                        let ty = self.resolve_type_expression(arg, context);
                        self.create_type_expression(ty)
                    } else {
                        arg.clone()
                    }
                })
                .collect();
            make_type(TypeKind::Meta(Box::new(make_type(TypeKind::Custom(
                name.clone(),
                Some(resolved_args),
            )))))
        } else {
            self.report_error("Expected generic type".to_string(), span);
            make_type(TypeKind::Error)
        }
    }

    pub(crate) fn infer_statement_type(&mut self, stmt: &Statement, context: &mut Context) -> Type {
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
}
