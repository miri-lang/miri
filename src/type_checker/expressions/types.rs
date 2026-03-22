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
                        span,
                    )),
                    Box::new(ast_factory::expr_with_span(
                        ExpressionKind::Type(
                            Box::new(ast_factory::make_type(TypeKind::Void)),
                            false,
                        ),
                        span,
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
                        span,
                    )),
                    Box::new(ast_factory::expr_with_span(
                        ExpressionKind::Type(Box::new(val_type), false),
                        span,
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
                                span,
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
                                            val.span,
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
                                        val.span,
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
                        let params = if let Some(p) = func_data.generics.as_ref() {
                            p
                        } else {
                            // Should not happen since we matched on `is_some()`
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

                        return make_type(TypeKind::Function(Box::new(FunctionTypeData {
                            generics: None,
                            params: new_params,
                            return_type: new_ret,
                        })));
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
