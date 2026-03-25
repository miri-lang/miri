// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Generic type handling for the type checker.
//!
//! This module handles:
//! - Generic type inference from arguments
//! - Type substitution with generic mappings
//! - Generic constraint validation
//! - Generic parameter definition

use super::context::{Context, GenericDefinition, TypeDefinition};
use super::TypeChecker;
use crate::ast::factory::make_type;
use crate::ast::types::{Type, TypeKind};
use crate::ast::{Expression, ExpressionKind};
use crate::error::syntax::Span;
use std::collections::HashMap;

impl TypeChecker {
    /// Infers generic type parameters from argument types.
    ///
    /// Given a parameter type (which may contain generic placeholders) and an
    /// argument type, this function infers what concrete types should be
    /// substituted for the generic parameters.
    ///
    /// # Example
    /// If `param_type` is `List<T>` and `arg_type` is `List<i32>`,
    /// this will add `T -> i32` to the mapping.
    pub(crate) fn infer_generic_types(
        &self,
        param_type: &Type,
        arg_type: &Type,
        mapping: &mut HashMap<String, Type>,
    ) {
        match (&param_type.kind, &arg_type.kind) {
            // Direct generic match
            (TypeKind::Generic(name, _, _), _) => {
                if !mapping.contains_key(name) {
                    mapping.insert(name.clone(), arg_type.clone());
                }
            }

            // Collection canonical variants are normalized to Custom before type-checking.
            // After normalization, List<T>/Map<K,V>/Set<T> all appear as Custom(...), so
            // they are handled by the Custom arm below.
            (
                TypeKind::List(_) | TypeKind::Map(_, _) | TypeKind::Set(_) | TypeKind::Array(_, _),
                _,
            ) => {
                unreachable!("collection types are normalized to Custom before this point")
            }

            // Option<T> matches Option<concrete>
            (TypeKind::Option(p_inner), TypeKind::Option(a_inner)) => {
                self.infer_generic_types(p_inner, a_inner, mapping);
            }

            // Custom<Args...> matches Custom<ConcreteArgs...>
            (TypeKind::Custom(p_name, p_args), TypeKind::Custom(a_name, a_args)) => {
                if p_name == a_name {
                    if let (Some(p_args), Some(a_args)) = (p_args, a_args) {
                        if p_args.len() == a_args.len() {
                            for (p_arg_expr, a_arg_expr) in p_args.iter().zip(a_args.iter()) {
                                if let (Ok(p_arg), Ok(a_arg)) = (
                                    self.extract_type_from_expression(p_arg_expr),
                                    self.extract_type_from_expression(a_arg_expr),
                                ) {
                                    self.infer_generic_types(&p_arg, &a_arg, mapping);
                                }
                            }
                        }
                    }
                }
            }

            _ => {}
        }
    }

    /// Substitutes generic type parameters with concrete types.
    ///
    /// Given a type containing generic parameters and a mapping from
    /// parameter names to concrete types, returns a new type with
    /// all generic parameters replaced.
    pub(crate) fn substitute_type(&self, ty: &Type, mapping: &HashMap<String, Type>) -> Type {
        match &ty.kind {
            // Direct substitution for generic types
            TypeKind::Generic(name, _, _) => {
                mapping.get(name).cloned().unwrap_or_else(|| ty.clone())
            }

            // Custom types - substitute name if it's a generic parameter, and recurse into args
            TypeKind::Custom(name, args) => {
                // Check if the type name itself is a generic parameter
                if args.is_none() {
                    if let Some(subst) = mapping.get(name) {
                        return subst.clone();
                    }
                }

                // Substitute in generic arguments
                let new_args = args.as_ref().map(|args_vec| {
                    args_vec
                        .iter()
                        .map(|arg| {
                            let arg_type = self
                                .extract_type_from_expression(arg)
                                .unwrap_or(make_type(TypeKind::Error));
                            let subst_arg = self.substitute_type(&arg_type, mapping);
                            self.create_type_expression(subst_arg)
                        })
                        .collect()
                });

                make_type(TypeKind::Custom(name.clone(), new_args))
            }

            // Collection canonical variants are normalized to Custom before type-checking.
            TypeKind::List(_) | TypeKind::Map(_, _) | TypeKind::Set(_) | TypeKind::Array(_, _) => {
                unreachable!("collection types are normalized to Custom before this point")
            }

            TypeKind::Option(inner) => make_type(TypeKind::Option(Box::new(
                self.substitute_type(inner, mapping),
            ))),

            TypeKind::Result(ok_expr, err_expr) => {
                if let (Ok(ok), Ok(err)) = (
                    self.extract_type_from_expression(ok_expr),
                    self.extract_type_from_expression(err_expr),
                ) {
                    make_type(TypeKind::Result(
                        Box::new(self.create_type_expression(self.substitute_type(&ok, mapping))),
                        Box::new(self.create_type_expression(self.substitute_type(&err, mapping))),
                    ))
                } else {
                    ty.clone()
                }
            }

            // Non-generic types pass through unchanged
            _ => ty.clone(),
        }
    }

    /// Validates that provided generic arguments satisfy their constraints.
    pub(crate) fn validate_generics(
        &mut self,
        args: &Option<Vec<Expression>>,
        params: &Option<Vec<GenericDefinition>>,
        context: &Context,
        span: Span,
    ) {
        let args_len = args.as_ref().map_or(0, |v| v.len());
        let params_len = params.as_ref().map_or(0, |v| v.len());

        if args_len != params_len {
            // Allow bare class name references inside own class body.
            // e.g., inside `class List<T>`, a parameter typed as `List` (without `<T>`)
            // is valid — it refers to the current class type.
            if args_len == 0 && context.current_class.is_some() {
                return;
            }
            self.report_error(
                format!(
                    "Generic argument count mismatch: expected {}, got {}",
                    params_len, args_len
                ),
                span,
            );
            return;
        }

        if let (Some(args_vec), Some(params_vec)) = (args, params) {
            for (i, arg_expr) in args_vec.iter().enumerate() {
                let param_def = &params_vec[i];
                let arg_type = self.resolve_type_expression(arg_expr, context);

                if let Some(constraint) = &param_def.constraint {
                    if !self.satisfies_constraint(&arg_type, constraint, &param_def.kind, context) {
                        self.report_error(
                            format!(
                                "Type {} does not satisfy constraint {} {}",
                                arg_type, param_def.kind, constraint
                            ),
                            arg_expr.span,
                        );
                    }
                }
            }
        }
    }

    /// Defines generic type parameters in the current scope.
    ///
    /// This is called when entering a generic function or type definition
    /// to make the generic parameters available for type resolution.
    pub(crate) fn define_generics(&mut self, generics: &[Expression], context: &mut Context) {
        for gen in generics {
            if let ExpressionKind::GenericType(name_expr, constraint_expr, kind) = &gen.node {
                let name = match &name_expr.node {
                    ExpressionKind::Identifier(n, _) => n.clone(),
                    _ => continue,
                };

                let constraint_type = constraint_expr
                    .as_ref()
                    .map(|c| self.resolve_type_expression(c, context));

                context.define_type(
                    name.clone(),
                    TypeDefinition::Generic(GenericDefinition {
                        name: name.clone(),
                        constraint: constraint_type,
                        kind: kind.clone(),
                    }),
                );
            }
        }
    }
}
