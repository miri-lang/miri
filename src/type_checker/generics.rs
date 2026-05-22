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
use crate::ast::common::Parameter;
use crate::ast::factory::make_type;
use crate::ast::types::{FunctionTypeData, Type, TypeKind};
use crate::ast::{Expression, ExpressionKind};
use crate::error::syntax::Span;
use std::collections::HashMap;

/// Sentinel `TypeKind::Custom` name used to smuggle a value-generic argument
/// (e.g. the `3` in `Foo<float, 3>`) through the existing
/// `HashMap<String, Type>` substitution map. Generic params can be either
/// type-typed or value-typed in their declared class, but `substitute_type`
/// is keyed by name → `Type`; wrapping the value expression as
/// `Custom("__value_generic__", Some([expr]))` lets us look up value
/// generics out of the same map in size-expression positions
/// (`substitute_value_generic_in_expr`) without threading a second mapping
/// through every callsite of `substitute_type`.
pub(crate) const VALUE_GENERIC_MARKER: &str = "__value_generic__";

/// Wrap a value-generic argument expression as a sentinel `Type` so it can
/// share the `HashMap<String, Type>` mapping used for type generics.
pub(crate) fn value_generic_marker_type(expr: Expression) -> Type {
    make_type(TypeKind::Custom(
        VALUE_GENERIC_MARKER.to_string(),
        Some(vec![expr]),
    ))
}

/// If `ty` is a value-generic marker wrapping a stored expression, return
/// a borrow of that expression. Otherwise `None`.
pub(crate) fn extract_value_generic(ty: &Type) -> Option<&Expression> {
    match &ty.kind {
        TypeKind::Custom(name, Some(args)) if name == VALUE_GENERIC_MARKER && args.len() == 1 => {
            Some(&args[0])
        }
        _ => None,
    }
}

/// Walk `expr` and substitute identifier references that name a value generic
/// in `mapping` with the stored expression. Identifiers that resolve to type
/// generics are left untouched — those flow through `substitute_type`. Used
/// by `substitute_array` (and any other size-position substitution) so that
/// e.g. `Array<T, Size>` inside a class body lowers to `Array<float, 3>` when
/// the class is instantiated as `Wrap<float, 3>`.
pub(crate) fn substitute_value_generic_in_expr(
    expr: &Expression,
    mapping: &HashMap<String, Type>,
) -> Expression {
    if let ExpressionKind::Identifier(name, None) = &expr.node {
        if let Some(ty) = mapping.get(name) {
            if let Some(value_expr) = extract_value_generic(ty) {
                return value_expr.clone();
            }
        }
    }
    expr.clone()
}

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
            (TypeKind::Generic(name, _, _), _) if !mapping.contains_key(name) => {
                mapping.insert(name.clone(), arg_type.clone());
            }

            // Unnormalized collection variants
            (TypeKind::List(elem), _) => {
                self.infer_unnormalized_list(elem, arg_type, mapping);
            }
            (TypeKind::Map(k, v), _) => {
                self.infer_unnormalized_map(k, v, arg_type, mapping);
            }
            (TypeKind::Set(elem), _) => {
                self.infer_unnormalized_set(elem, arg_type, mapping);
            }
            (TypeKind::Array(elem, size), _) => {
                self.infer_unnormalized_array(elem, size, arg_type, mapping);
            }

            // Tuple<T, U, ...> matches Tuple<concrete, concrete, ...>
            (TypeKind::Tuple(p_elems), TypeKind::Tuple(a_elems))
                if p_elems.len() == a_elems.len() =>
            {
                self.infer_tuple_generics(p_elems, a_elems, mapping);
            }

            // Option<T> matches Option<concrete>
            (TypeKind::Option(p_inner), TypeKind::Option(a_inner)) => {
                self.infer_generic_types(p_inner, a_inner, mapping);
            }

            // Custom<Args...> matches Custom<ConcreteArgs...>
            (TypeKind::Custom(p_name, p_args), TypeKind::Custom(a_name, a_args))
                if p_name == a_name =>
            {
                self.infer_custom_generics(p_args, a_args, mapping);
            }

            // fn(T) R matches fn(concrete) concrete
            (TypeKind::Function(p_func), TypeKind::Function(a_func)) => {
                self.infer_function_generics(p_func, a_func, mapping);
            }

            _ => {}
        }
    }

    fn infer_unnormalized_list(
        &self,
        elem: &Expression,
        arg_type: &Type,
        mapping: &mut HashMap<String, Type>,
    ) {
        let normalized = make_type(TypeKind::Custom(
            "List".to_string(),
            Some(vec![elem.clone()]),
        ));
        self.infer_generic_types(&normalized, arg_type, mapping);
    }

    fn infer_unnormalized_map(
        &self,
        k: &Expression,
        v: &Expression,
        arg_type: &Type,
        mapping: &mut HashMap<String, Type>,
    ) {
        let normalized = make_type(TypeKind::Custom(
            "Map".to_string(),
            Some(vec![k.clone(), v.clone()]),
        ));
        self.infer_generic_types(&normalized, arg_type, mapping);
    }

    fn infer_unnormalized_set(
        &self,
        elem: &Expression,
        arg_type: &Type,
        mapping: &mut HashMap<String, Type>,
    ) {
        let normalized = make_type(TypeKind::Custom(
            "Set".to_string(),
            Some(vec![elem.clone()]),
        ));
        self.infer_generic_types(&normalized, arg_type, mapping);
    }

    fn infer_unnormalized_array(
        &self,
        elem: &Expression,
        size: &Expression,
        arg_type: &Type,
        mapping: &mut HashMap<String, Type>,
    ) {
        let normalized = make_type(TypeKind::Custom(
            "Array".to_string(),
            Some(vec![elem.clone(), size.clone()]),
        ));
        self.infer_generic_types(&normalized, arg_type, mapping);
    }

    fn infer_tuple_generics(
        &self,
        p_elems: &[Expression],
        a_elems: &[Expression],
        mapping: &mut HashMap<String, Type>,
    ) {
        for (p_elem_expr, a_elem_expr) in p_elems.iter().zip(a_elems.iter()) {
            if let (Ok(p_elem), Ok(a_elem)) = (
                self.extract_type_from_expression(p_elem_expr),
                self.extract_type_from_expression(a_elem_expr),
            ) {
                self.infer_generic_types(&p_elem, &a_elem, mapping);
            }
        }
    }

    fn infer_custom_generics(
        &self,
        p_args: &Option<Vec<Expression>>,
        a_args: &Option<Vec<Expression>>,
        mapping: &mut HashMap<String, Type>,
    ) {
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

    fn infer_function_generics(
        &self,
        p_func: &FunctionTypeData,
        a_func: &FunctionTypeData,
        mapping: &mut HashMap<String, Type>,
    ) {
        for (p_param, a_param) in p_func.params.iter().zip(a_func.params.iter()) {
            if let (Ok(p_ty), Ok(a_ty)) = (
                self.extract_type_from_expression(&p_param.typ),
                self.extract_type_from_expression(&a_param.typ),
            ) {
                self.infer_generic_types(&p_ty, &a_ty, mapping);
            }
        }
        if let (Some(p_rt), Some(a_rt)) = (&p_func.return_type, &a_func.return_type) {
            if let (Ok(p_ty), Ok(a_ty)) = (
                self.extract_type_from_expression(p_rt),
                self.extract_type_from_expression(a_rt),
            ) {
                self.infer_generic_types(&p_ty, &a_ty, mapping);
            }
        }
    }

    /// Substitutes generic type parameters with concrete types.
    ///
    /// Given a type containing generic parameters and a mapping from
    /// parameter names to concrete types, returns a new type with
    /// all generic parameters replaced.
    pub(crate) fn substitute_type(&self, ty: &Type, mapping: &HashMap<String, Type>) -> Type {
        match &ty.kind {
            TypeKind::Generic(name, _, _) => {
                mapping.get(name).cloned().unwrap_or_else(|| ty.clone())
            }
            TypeKind::Custom(name, args) => self.substitute_custom(name, args, mapping),
            TypeKind::List(elem_expr) => self.substitute_list(elem_expr, mapping),
            TypeKind::Map(k_expr, v_expr) => self.substitute_map(k_expr, v_expr, mapping),
            TypeKind::Set(elem_expr) => self.substitute_set(elem_expr, mapping),
            TypeKind::Array(elem_expr, size_expr) => {
                self.substitute_array(elem_expr, size_expr, mapping)
            }
            TypeKind::Option(inner) => self.substitute_option(inner, mapping),
            TypeKind::Tuple(elements) => self.substitute_tuple(elements, mapping),
            TypeKind::Result(ok_expr, err_expr) => {
                self.substitute_result(ok_expr, err_expr, mapping)
            }
            TypeKind::Function(func) => self.substitute_function(func, mapping),
            _ => ty.clone(),
        }
    }

    fn substitute_custom(
        &self,
        name: &str,
        args: &Option<Vec<Expression>>,
        mapping: &HashMap<String, Type>,
    ) -> Type {
        if args.is_none() {
            if let Some(subst) = mapping.get(name) {
                // Bare value-generic reference (e.g. `Size` parsed as a type
                // name inside a position the type checker reaches): pull the
                // original value expression out of the marker so the caller
                // doesn't see the synthetic `Custom("__value_generic__", …)`.
                if let Some(value_expr) = extract_value_generic(subst) {
                    if let Ok(value_ty) = self.extract_type_from_expression(value_expr) {
                        return value_ty;
                    }
                }
                return subst.clone();
            }
        }

        let new_args = args.as_ref().map(|args_vec| {
            args_vec
                .iter()
                .map(|arg| {
                    if self.extract_type_from_expression(arg).is_ok() {
                        let arg_type = self
                            .extract_type_from_expression(arg)
                            .unwrap_or(make_type(TypeKind::Error));
                        let subst_arg = self.substitute_type(&arg_type, mapping);
                        self.create_type_expression(subst_arg)
                    } else {
                        // Value-generic position (e.g. the `Size` slot in a
                        // resolved `Custom("Array", [..., Identifier("Size")])`).
                        // Rewrite identifier references through the value-generic
                        // marker mapping and keep the expression verbatim
                        // otherwise.
                        substitute_value_generic_in_expr(arg, mapping)
                    }
                })
                .collect()
        });

        make_type(TypeKind::Custom(name.to_string(), new_args))
    }

    fn substitute_list(&self, elem_expr: &Expression, mapping: &HashMap<String, Type>) -> Type {
        let elem = self
            .extract_type_from_expression(elem_expr)
            .unwrap_or(make_type(TypeKind::Error));
        let subst_elem = self.substitute_type(&elem, mapping);
        make_type(TypeKind::Custom(
            "List".to_string(),
            Some(vec![self.create_type_expression(subst_elem)]),
        ))
    }

    fn substitute_map(
        &self,
        k_expr: &Expression,
        v_expr: &Expression,
        mapping: &HashMap<String, Type>,
    ) -> Type {
        let k = self
            .extract_type_from_expression(k_expr)
            .unwrap_or(make_type(TypeKind::Error));
        let v = self
            .extract_type_from_expression(v_expr)
            .unwrap_or(make_type(TypeKind::Error));
        let subst_k = self.substitute_type(&k, mapping);
        let subst_v = self.substitute_type(&v, mapping);
        make_type(TypeKind::Custom(
            "Map".to_string(),
            Some(vec![
                self.create_type_expression(subst_k),
                self.create_type_expression(subst_v),
            ]),
        ))
    }

    fn substitute_set(&self, elem_expr: &Expression, mapping: &HashMap<String, Type>) -> Type {
        let elem = self
            .extract_type_from_expression(elem_expr)
            .unwrap_or(make_type(TypeKind::Error));
        let subst_elem = self.substitute_type(&elem, mapping);
        make_type(TypeKind::Custom(
            "Set".to_string(),
            Some(vec![self.create_type_expression(subst_elem)]),
        ))
    }

    fn substitute_array(
        &self,
        elem_expr: &Expression,
        size_expr: &Expression,
        mapping: &HashMap<String, Type>,
    ) -> Type {
        let elem = self
            .extract_type_from_expression(elem_expr)
            .unwrap_or(make_type(TypeKind::Error));
        let subst_elem = self.substitute_type(&elem, mapping);
        let subst_size = substitute_value_generic_in_expr(size_expr, mapping);
        make_type(TypeKind::Custom(
            "Array".to_string(),
            Some(vec![self.create_type_expression(subst_elem), subst_size]),
        ))
    }

    fn substitute_option(&self, inner: &Type, mapping: &HashMap<String, Type>) -> Type {
        make_type(TypeKind::Option(Box::new(
            self.substitute_type(inner, mapping),
        )))
    }

    fn substitute_tuple(&self, elements: &[Expression], mapping: &HashMap<String, Type>) -> Type {
        let new_elements = elements
            .iter()
            .map(|elem_expr| {
                let elem = self
                    .extract_type_from_expression(elem_expr)
                    .unwrap_or(make_type(TypeKind::Error));
                let subst = self.substitute_type(&elem, mapping);
                self.create_type_expression(subst)
            })
            .collect();
        make_type(TypeKind::Tuple(new_elements))
    }

    fn substitute_result(
        &self,
        ok_expr: &Expression,
        err_expr: &Expression,
        mapping: &HashMap<String, Type>,
    ) -> Type {
        if let (Ok(ok), Ok(err)) = (
            self.extract_type_from_expression(ok_expr),
            self.extract_type_from_expression(err_expr),
        ) {
            make_type(TypeKind::Result(
                Box::new(self.create_type_expression(self.substitute_type(&ok, mapping))),
                Box::new(self.create_type_expression(self.substitute_type(&err, mapping))),
            ))
        } else {
            make_type(TypeKind::Result(
                Box::new(ok_expr.clone()),
                Box::new(err_expr.clone()),
            ))
        }
    }

    fn substitute_function(
        &self,
        func: &FunctionTypeData,
        mapping: &HashMap<String, Type>,
    ) -> Type {
        let new_params: Vec<Parameter> = func
            .params
            .iter()
            .map(|p| {
                let param_type = self
                    .extract_type_from_expression(&p.typ)
                    .unwrap_or(make_type(TypeKind::Error));
                let subst = self.substitute_type(&param_type, mapping);
                Parameter {
                    typ: Box::new(self.create_type_expression(subst)),
                    ..p.clone()
                }
            })
            .collect();
        let new_return = func.return_type.as_ref().map(|rt_expr| {
            let rt = self
                .extract_type_from_expression(rt_expr)
                .unwrap_or(make_type(TypeKind::Error));
            let subst = self.substitute_type(&rt, mapping);
            Box::new(self.create_type_expression(subst))
        });
        make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: func.generics.clone(),
            params: new_params,
            return_type: new_return,
        })))
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
                // Skip value-generic args (e.g. the `3` in `Foo<float, 3>`):
                // they aren't type expressions, so `resolve_type_expression`
                // would report "Expected type expression". Constraints on
                // value generics aren't modeled today; trying to validate one
                // as if it were a type produces a spurious error at every
                // instantiation site.
                if self.extract_type_from_expression(arg_expr).is_err() {
                    continue;
                }
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
                        kind: *kind,
                    }),
                );
            }
        }
    }
}
