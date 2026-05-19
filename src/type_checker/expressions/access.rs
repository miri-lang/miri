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
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::ast::*;
use crate::error::format::find_best_match;
use crate::error::syntax::Span;
use crate::type_checker::context::{Context, TypeDefinition};
use crate::type_checker::TypeChecker;
use std::collections::HashMap;

impl TypeChecker {
    /// Infers the type of an index access expression (`obj[index]`).
    ///
    /// Supports list, map, tuple, string, and array indexing, as well as
    /// range-based slicing for lists, strings, and homogeneous tuples.
    pub(crate) fn infer_index(
        &mut self,
        obj: &Expression,
        index: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let obj_type = self.infer_expression(obj, context);

        if matches!(obj_type.kind, TypeKind::Error) {
            return make_type(TypeKind::Error);
        }

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
                                index.span,
                            );
                            return make_type(TypeKind::Error);
                        }
                    }
                }

                return self.infer_index_with_range(&obj_type, index, span, context);
            }
        }

        self.infer_index_regular(&obj_type, index, &index_type, span, context)
    }

    fn infer_index_with_range(
        &mut self,
        obj_type: &Type,
        index: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        match obj_type.kind {
            TypeKind::String => make_type(TypeKind::String),
            TypeKind::List(_) | TypeKind::Array(_, _) => {
                unreachable!("collection types are normalized to Custom before this point")
            }
            TypeKind::Custom(ref cname, ref cargs)
                if matches!(
                    BuiltinCollectionKind::from_name(cname.as_str()),
                    Some(BuiltinCollectionKind::List)
                ) =>
            {
                make_type(TypeKind::Custom(cname.clone(), cargs.clone()))
            }
            TypeKind::Custom(ref cname, ref cargs)
                if BuiltinCollectionKind::from_name(cname.as_str())
                    == Some(BuiltinCollectionKind::Array) =>
            {
                self.infer_index_with_range_array(cname, cargs, index, span, context)
            }
            TypeKind::Tuple(ref elements) => {
                self.infer_index_with_range_tuple(elements, span, context)
            }
            _ => {
                self.report_error(format!("Type {} is not sliceable", obj_type), span);
                make_type(TypeKind::Error)
            }
        }
    }

    fn infer_index_with_range_array(
        &mut self,
        cname: &str,
        cargs: &Option<Vec<Expression>>,
        index: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let inner = match cargs.as_deref() {
            Some([i, ..]) => i.clone(),
            _ => return make_type(TypeKind::Error),
        };
        let size_expr_opt = match cargs.as_deref() {
            Some([_, s, ..]) => Some(s.clone()),
            _ => None,
        };
        if let Some(size_expr) = size_expr_opt {
            let array_size = Self::try_eval_const_int(&size_expr);
            self.check_slice_bounds_for_array(array_size, index, span, context);
            if let ExpressionKind::Range(start, end, range_type) = &index.node {
                let start_val = Self::try_eval_const_int_with_context(start, context);
                let end_val = end
                    .as_ref()
                    .and_then(|e| Self::try_eval_const_int_with_context(e, context));
                if let (Some(s), Some(e)) = (start_val, end_val) {
                    let slice_size = match range_type {
                        RangeExpressionType::Exclusive => e - s,
                        RangeExpressionType::Inclusive => e - s + 1,
                        _ => e - s,
                    };
                    let new_size = crate::ast::factory::int_literal_expression(slice_size);
                    return make_type(TypeKind::Custom(
                        "Array".to_string(),
                        Some(vec![inner, new_size]),
                    ));
                }
            }
            return make_type(TypeKind::Custom(
                cname.to_string(),
                Some(vec![inner, size_expr]),
            ));
        }
        make_type(TypeKind::Custom(cname.to_string(), cargs.clone()))
    }

    fn infer_index_with_range_tuple(
        &mut self,
        elements: &[Expression],
        span: Span,
        context: &mut Context,
    ) -> Type {
        if elements.is_empty() {
            return make_type(TypeKind::Custom(
                "List".to_string(),
                Some(vec![self.create_type_expression(make_type(TypeKind::Void))]),
            ));
        }
        let first = self.resolve_type_expression(&elements[0], context);
        let is_homogeneous = elements.iter().all(|e| {
            let t = self.resolve_type_expression(e, context);
            self.are_compatible(&t, &first, context)
        });

        if is_homogeneous {
            make_type(TypeKind::Custom(
                "List".to_string(),
                Some(vec![self.create_type_expression(first)]),
            ))
        } else {
            self.report_error("Cannot slice heterogeneous tuple".to_string(), span);
            make_type(TypeKind::Error)
        }
    }

    fn check_slice_bounds_for_array(
        &mut self,
        array_size: Option<i128>,
        index: &Expression,
        span: Span,
        context: &mut Context,
    ) {
        if let ExpressionKind::Range(start, end, _range_type) = &index.node {
            if let Some(size) = array_size {
                if let Some(start_val) = Self::try_eval_const_int_with_context(start, context) {
                    if start_val < 0 {
                        self.report_error(
                            "Slice start index must be a non-negative integer".to_string(),
                            start.span,
                        );
                    } else if start_val > size {
                        self.report_error(
                            format!(
                                "Slice start index out of bounds: index {} but array has {} elements",
                                start_val, size
                            ),
                            start.span,
                        );
                    }
                }
                if let Some(end_expr) = end {
                    if let Some(end_val) = Self::try_eval_const_int_with_context(end_expr, context)
                    {
                        if end_val < 0 {
                            self.report_error(
                                "Slice end index must be a non-negative integer".to_string(),
                                end_expr.span,
                            );
                        } else if end_val > size {
                            self.report_error(
                                format!(
                                    "Slice end index out of bounds: index {} but array has {} elements",
                                    end_val, size
                                ),
                                end_expr.span,
                            );
                        }
                    }
                }
            }

            if let Some(end_expr) = end {
                if let (Some(s), Some(e)) = (
                    Self::try_eval_const_int_with_context(start, context),
                    Self::try_eval_const_int_with_context(end_expr, context),
                ) {
                    if s > e {
                        self.report_error(
                            format!(
                                "Slice start index ({}) is greater than end index ({})",
                                s, e
                            ),
                            span,
                        );
                    }
                }
            }
        }
    }

    fn infer_index_regular(
        &mut self,
        obj_type: &Type,
        index: &Expression,
        index_type: &Type,
        span: Span,
        context: &mut Context,
    ) -> Type {
        match obj_type.kind {
            TypeKind::Array(_, _) | TypeKind::List(_) | TypeKind::Map(_, _) => {
                unreachable!("collection types are normalized to Custom before this point")
            }
            TypeKind::Custom(ref name, ref args)
                if matches!(
                    BuiltinCollectionKind::from_name(name.as_str()),
                    Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
                ) || name == "Tuple" =>
            {
                self.infer_index_array_list_tuple(name, args, index, index_type, span, context)
            }
            TypeKind::Custom(ref name, ref args)
                if BuiltinCollectionKind::from_name(name.as_str())
                    == Some(BuiltinCollectionKind::Map) =>
            {
                self.infer_index_map(args, index, index_type, context)
            }
            TypeKind::Tuple(ref element_type_exprs) => {
                self.infer_index_tuple_hetero(element_type_exprs, index, index_type, span, context)
            }
            TypeKind::String => {
                if !matches!(index_type.kind, TypeKind::Int) {
                    self.report_error("String index must be an integer".to_string(), index.span);
                    return make_type(TypeKind::Error);
                }
                make_type(TypeKind::String)
            }
            TypeKind::Error => make_type(TypeKind::Error),
            _ => {
                self.report_error(format!("Type {} is not indexable", obj_type), span);
                make_type(TypeKind::Error)
            }
        }
    }

    fn infer_index_array_list_tuple(
        &mut self,
        name: &str,
        args: &Option<Vec<Expression>>,
        index: &Expression,
        index_type: &Type,
        span: Span,
        context: &mut Context,
    ) -> Type {
        if !matches!(index_type.kind, TypeKind::Int) {
            self.report_error(format!("{} index must be an integer", name), index.span);
            return make_type(TypeKind::Error);
        }

        if BuiltinCollectionKind::from_name(name) == Some(BuiltinCollectionKind::Array) {
            if let Some(idx_val) = Self::try_eval_const_int_with_context(index, context) {
                if idx_val < 0 {
                    self.report_error(
                        "Array index must be a non-negative integer".to_string(),
                        index.span,
                    );
                    return make_type(TypeKind::Error);
                }
                if let Some(size_expr) = args.as_deref().and_then(|a| a.get(1)) {
                    if let Some(size_val) = Self::try_eval_const_int(size_expr) {
                        let idx = idx_val as usize;
                        let size = size_val as usize;
                        if idx >= size {
                            self.report_error(
                                format!(
                                    "Array index out of bounds: index {} but array has {} elements",
                                    idx, size
                                ),
                                span,
                            );
                            return make_type(TypeKind::Error);
                        }
                    }
                }
            }
        }

        if let Some(args) = args {
            if let Some(inner_type_expr) = args.first() {
                return self.resolve_type_expression(inner_type_expr, context);
            }
        } else if let Some(TypeDefinition::Generic(g)) = context.resolve_type_definition("T") {
            return make_type(TypeKind::Generic(
                g.name.clone(),
                g.constraint.clone().map(Box::new),
                g.kind,
            ));
        } else {
            return make_type(TypeKind::Generic(
                "T".to_string(),
                None,
                TypeDeclarationKind::None,
            ));
        }
        make_type(TypeKind::Error)
    }

    fn infer_index_map(
        &mut self,
        args: &Option<Vec<Expression>>,
        index: &Expression,
        index_type: &Type,
        context: &mut Context,
    ) -> Type {
        if let Some(args) = args {
            if args.len() >= 2 {
                let key_type = self.resolve_type_expression(&args[0], context);
                if !self.are_compatible(&key_type, index_type, context) {
                    self.report_error("Invalid map key type".to_string(), index.span);
                    return make_type(TypeKind::Error);
                }
                return self.resolve_type_expression(&args[1], context);
            }
        }
        if let Some(TypeDefinition::Generic(g)) = context.resolve_type_definition("V") {
            make_type(TypeKind::Generic(
                g.name.clone(),
                g.constraint.clone().map(Box::new),
                g.kind,
            ))
        } else {
            make_type(TypeKind::Generic(
                "V".to_string(),
                None,
                TypeDeclarationKind::None,
            ))
        }
    }

    fn infer_index_tuple_hetero(
        &mut self,
        element_type_exprs: &[Expression],
        index: &Expression,
        index_type: &Type,
        span: Span,
        context: &mut Context,
    ) -> Type {
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
                self.report_error("Tuple index must be an integer".to_string(), index.span);
                return make_type(TypeKind::Error);
            }
            if element_type_exprs.is_empty() {
                self.report_error("Tuple index out of bounds (empty tuple)".to_string(), span);
                return make_type(TypeKind::Error);
            }

            if let ExpressionKind::Literal(Literal::Integer(val)) = &index.node {
                let idx = val.to_usize();
                if idx >= element_type_exprs.len() {
                    self.report_error("Tuple index out of bounds".to_string(), span);
                    return make_type(TypeKind::Error);
                }
            }

            self.resolve_type_expression(&element_type_exprs[0], context)
        } else if let ExpressionKind::Literal(Literal::Integer(val)) = &index.node {
            let idx = val.to_usize();

            if idx < element_type_exprs.len() {
                self.resolve_type_expression(&element_type_exprs[idx], context)
            } else {
                self.report_error("Tuple index out of bounds".to_string(), span);
                make_type(TypeKind::Error)
            }
        } else {
            self.report_error(
                "Tuple index must be an integer literal for heterogeneous tuples".to_string(),
                index.span,
            );
            make_type(TypeKind::Error)
        }
    }

    /// Infers the type of a member access expression (`obj.prop`).
    ///
    /// Handles tuple indexing, struct/class field access, enum variant access,
    /// option/result built-in methods, and inheritance chain lookup.
    pub(crate) fn infer_member(
        &mut self,
        obj: &Expression,
        prop: &Expression,
        span: Span,
        context: &mut Context,
    ) -> Type {
        if let ExpressionKind::Identifier(alias_name, _) = &obj.node {
            if let Some(module_path) = self.module_aliases.get(alias_name.as_str()).cloned() {
                return self.infer_member_module_alias(
                    alias_name,
                    &module_path,
                    obj,
                    prop,
                    span,
                    context,
                );
            }
        }

        let obj_type = self.infer_expression(obj, context);

        if matches!(obj_type.kind, TypeKind::Error) {
            return make_type(TypeKind::Error);
        }

        if let TypeKind::Tuple(element_types) = &obj_type.kind {
            if let ExpressionKind::Literal(Literal::Integer(val)) = &prop.node {
                let idx = val.to_usize();
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
                prop.span,
            );
            return make_type(TypeKind::Error);
        };

        self.infer_member_dispatch(&obj_type, prop_name, span, context)
    }

    fn infer_member_dispatch(
        &mut self,
        obj_type: &Type,
        prop_name: &str,
        span: Span,
        context: &mut Context,
    ) -> Type {
        let (type_name, type_args) = self.extract_member_type_and_args(obj_type, span, context);

        if let Some(name) = &type_name {
            if name == "Kernel" && prop_name == "launch" {
                return self.infer_member_kernel_launch();
            }
        }

        if let Some(name) = type_name {
            let def_opt = self
                .resolve_visible_type(&name, context)
                .or_else(|| {
                    if BuiltinCollectionKind::from_name(&name).is_some() {
                        self.global_type_definitions.get(&name)
                    } else {
                        None
                    }
                })
                .cloned();

            if let Some(TypeDefinition::Struct(def)) = &def_opt {
                return self.infer_member_struct(def, &name, prop_name, &type_args, span, context);
            } else if let Some(TypeDefinition::Class(def)) = &def_opt {
                return self.infer_member_class(def, &name, prop_name, &type_args, span, context);
            } else if let Some(TypeDefinition::Trait(trait_def)) = &def_opt {
                return self.infer_member_trait(&name, trait_def, prop_name, span, context);
            } else if let Some(TypeDefinition::Enum(enum_def)) = &def_opt {
                return self.infer_member_enum(enum_def, &name, prop_name, obj_type, span, context);
            } else if def_opt.is_none() {
                if let Some(module) = self.suggest_module_for_type(&name) {
                    self.report_error_with_help(
                        format!("Type '{}' does not have members", obj_type),
                        span,
                        format!("Consider importing '{}' to use {} methods", module, name),
                    );
                } else {
                    self.report_error(format!("Type '{}' does not have members", obj_type), span);
                }
                return make_type(TypeKind::Error);
            }
        }

        match &obj_type.kind {
            TypeKind::Meta(inner_type) => {
                self.infer_member_meta(inner_type, prop_name, span, context)
            }
            _ => {
                self.report_error(format!("Type '{}' does not have members", obj_type), span);
                make_type(TypeKind::Error)
            }
        }
    }

    fn infer_member_module_alias(
        &mut self,
        _alias_name: &str,
        module_path: &str,
        obj: &Expression,
        prop: &Expression,
        span: Span,
        _context: &mut Context,
    ) -> Type {
        let prop_name = if let ExpressionKind::Identifier(name, _) = &prop.node {
            name.clone()
        } else {
            self.report_error(
                "Member property must be an identifier".to_string(),
                prop.span,
            );
            return make_type(TypeKind::Error);
        };

        self.types.insert(obj.id, make_type(TypeKind::Identifier));

        if let Some(info) = self.global_scope.get(prop_name.as_str()).cloned() {
            if !self.check_visibility(&info.visibility, &info.module) {
                self.report_error(format!("'{}' is not visible", prop_name), span);
                return make_type(TypeKind::Error);
            }
            return info.ty.clone();
        }

        self.report_error(
            format!("'{}' is not defined in module '{}'", prop_name, module_path),
            span,
        );
        make_type(TypeKind::Error)
    }

    fn extract_member_type_and_args(
        &mut self,
        obj_type: &Type,
        span: Span,
        context: &mut Context,
    ) -> (Option<String>, Option<Vec<Expression>>) {
        match &obj_type.kind {
            TypeKind::String => (Some("String".to_string()), None),
            TypeKind::List(elem) => (Some("List".to_string()), Some(vec![*elem.clone()])),
            TypeKind::Map(k, v) => (Some("Map".to_string()), Some(vec![*k.clone(), *v.clone()])),
            TypeKind::Set(elem) => (Some("Set".to_string()), Some(vec![*elem.clone()])),
            TypeKind::Array(elem, size) => (
                Some("Array".to_string()),
                Some(vec![*elem.clone(), *size.clone()]),
            ),
            TypeKind::Tuple(element_type_exprs) => {
                if !element_type_exprs.is_empty() {
                    let resolved_types: Vec<Type> = element_type_exprs
                        .iter()
                        .map(|t| self.resolve_type_expression(t, context))
                        .collect();
                    let first_type = &resolved_types[0];
                    let is_homogeneous = resolved_types
                        .iter()
                        .all(|t| self.are_compatible(t, first_type, context));
                    if is_homogeneous {
                        (
                            Some("Tuple".to_string()),
                            Some(vec![self.create_type_expression(first_type.clone())]),
                        )
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                }
            }
            TypeKind::Custom(name, args) => (Some(name.clone()), args.clone()),
            TypeKind::Result(ok, err) => (
                Some("Result".to_string()),
                Some(vec![*ok.clone(), *err.clone()]),
            ),
            TypeKind::Option(_) => (None, None),
            TypeKind::Generic(_, Some(constraint), _) => match &constraint.kind {
                TypeKind::Custom(name, args) => (Some(name.clone()), args.clone()),
                _ => (None, None),
            },
            TypeKind::Generic(name, None, _) => {
                self.report_error(
                    format!(
                        "Generic type '{}' without constraints has no known members",
                        name
                    ),
                    span,
                );
                (None, None)
            }
            _ => (None, None),
        }
    }

    fn infer_member_kernel_launch(&mut self) -> Type {
        let dim3_type = ast_factory::make_type(TypeKind::Custom("Dim3".to_string(), None));
        let dim3_expr = Box::new(ast_factory::type_expr_non_null(dim3_type.clone()));

        let future_void_type = ast_factory::make_type(TypeKind::Custom(
            "Future".to_string(),
            Some(vec![ast_factory::type_expr_non_null(
                ast_factory::make_type(TypeKind::Void),
            )]),
        ));

        ast_factory::make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: None,
            params: vec![
                Parameter {
                    name: "grid".to_string(),
                    typ: dim3_expr.clone(),
                    guard: None,
                    default_value: None,
                    is_out: false,
                },
                Parameter {
                    name: "block".to_string(),
                    typ: dim3_expr,
                    guard: None,
                    default_value: None,
                    is_out: false,
                },
            ],
            return_type: Some(Box::new(ast_factory::type_expr_non_null(future_void_type))),
        })))
    }

    fn infer_member_struct(
        &mut self,
        def: &crate::type_checker::context::StructDefinition,
        type_name: &str,
        prop_name: &str,
        type_args: &Option<Vec<Expression>>,
        span: Span,
        _context: &mut Context,
    ) -> Type {
        if let Some((_, field_type, visibility)) =
            def.fields.iter().find(|(n, _, _)| n == prop_name)
        {
            if !self.check_visibility(visibility, &def.module) {
                self.report_error(format!("Field '{}' is not visible", prop_name), span);
                return make_type(TypeKind::Error);
            }

            if let Some(generics) = &def.generics {
                if let Some(type_args) = type_args {
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
        }

        let candidates: Vec<&str> = def.fields.iter().map(|(n, _, _)| n.as_str()).collect();
        if let Some(suggestion) = find_best_match(prop_name, &candidates) {
            self.report_error_with_help(
                format!("Type '{}' has no field '{}'", type_name, prop_name),
                span,
                format!("Did you mean '{}'?", suggestion),
            );
        } else {
            self.report_error(
                format!("Type '{}' has no field '{}'", type_name, prop_name),
                span,
            );
        }
        make_type(TypeKind::Error)
    }

    fn infer_member_class(
        &mut self,
        def: &crate::type_checker::context::ClassDefinition,
        name: &str,
        prop_name: &str,
        type_args: &Option<Vec<Expression>>,
        span: Span,
        context: &mut Context,
    ) -> Type {
        if let Some(ty) =
            self.search_class_hierarchy(def, name, prop_name, type_args, span, context)
        {
            return ty;
        }

        if let Some(ty) = self.infer_member_class_trait_fallback(name, prop_name, type_args) {
            return ty;
        }

        self.report_class_member_not_found(name, prop_name, context, span)
    }

    fn search_class_hierarchy(
        &mut self,
        def: &crate::type_checker::context::ClassDefinition,
        name: &str,
        prop_name: &str,
        type_args: &Option<Vec<Expression>>,
        span: Span,
        context: &mut Context,
    ) -> Option<Type> {
        let mut search_class_def: crate::type_checker::context::ClassDefinition = def.clone();
        loop {
            let mapping = self.build_class_method_mapping(&search_class_def, name, type_args);

            if let Some(field_ty) =
                self.lookup_class_field(&search_class_def, prop_name, name, &mapping, span, context)
            {
                return Some(field_ty);
            }

            if let Some(method_ty) = self.lookup_class_method(
                &search_class_def,
                prop_name,
                name,
                &mapping,
                span,
                context,
            ) {
                return Some(method_ty);
            }

            let next_def = search_class_def
                .base_class
                .as_ref()
                .and_then(|base_class_name| {
                    context
                        .resolve_type_definition(base_class_name)
                        .or_else(|| self.global_type_definitions.get(base_class_name))
                        .and_then(|def| {
                            if let TypeDefinition::Class(c) = def {
                                Some(c.clone())
                            } else {
                                None
                            }
                        })
                });

            match next_def {
                Some(base_def) => search_class_def = base_def,
                None => break,
            }
        }
        None
    }

    fn lookup_class_field(
        &mut self,
        search_class_def: &crate::type_checker::context::ClassDefinition,
        prop_name: &str,
        name: &str,
        mapping: &std::collections::HashMap<String, Type>,
        span: Span,
        context: &mut Context,
    ) -> Option<Type> {
        search_class_def
            .fields
            .iter()
            .find(|(n, _)| n == prop_name)
            .cloned()
            .map(|(_, field_info)| {
                if !self.check_member_visibility(
                    &field_info.visibility,
                    &search_class_def.name,
                    context.current_class.as_deref(),
                    Some(name),
                ) {
                    self.report_error(
                        format!(
                            "Field '{}' of class '{}' is {:?} and cannot be accessed from here",
                            prop_name, search_class_def.name, field_info.visibility
                        ),
                        span,
                    );
                    make_type(TypeKind::Error)
                } else if mapping.is_empty() {
                    field_info.ty.clone()
                } else {
                    self.substitute_type(&field_info.ty, mapping)
                }
            })
    }

    fn lookup_class_method(
        &mut self,
        search_class_def: &crate::type_checker::context::ClassDefinition,
        prop_name: &str,
        name: &str,
        mapping: &std::collections::HashMap<String, Type>,
        span: Span,
        context: &mut Context,
    ) -> Option<Type> {
        search_class_def
            .methods
            .get(prop_name)
            .cloned()
            .map(|method_info| {
                if !self.check_member_visibility(
                    &method_info.visibility,
                    &search_class_def.name,
                    context.current_class.as_deref(),
                    Some(name),
                ) {
                    self.report_error(
                        format!(
                            "Method '{}' of class '{}' is {:?} and cannot be accessed from here",
                            prop_name, search_class_def.name, method_info.visibility
                        ),
                        span,
                    );
                    make_type(TypeKind::Error)
                } else {
                    self.build_class_method_type(&method_info, mapping)
                }
            })
    }

    fn build_class_method_mapping(
        &mut self,
        search_class_def: &crate::type_checker::context::ClassDefinition,
        name: &str,
        type_args: &Option<Vec<Expression>>,
    ) -> std::collections::HashMap<String, Type> {
        let mut mapping = std::collections::HashMap::new();
        if let Some(generics) = &search_class_def.generics {
            if let Some(type_args) = type_args {
                if generics.len() == type_args.len() {
                    for (param, arg_expr) in generics.iter().zip(type_args.iter()) {
                        let arg_type = self
                            .extract_type_from_expression(arg_expr)
                            .unwrap_or(make_type(TypeKind::Error));
                        mapping.insert(param.name.clone(), arg_type);
                    }
                } else {
                    let orig_generics_opt = self
                        .global_type_definitions
                        .get(name)
                        .and_then(|td| {
                            if let TypeDefinition::Class(cd) = td {
                                cd.generics.as_ref()
                            } else {
                                None
                            }
                        })
                        .cloned();
                    if let Some(orig_generics) = orig_generics_opt {
                        for base_generic in generics.iter() {
                            if let Some(idx) = orig_generics
                                .iter()
                                .position(|g| g.name == base_generic.name)
                            {
                                if let Some(arg_expr) = type_args.get(idx) {
                                    let arg_type = self
                                        .extract_type_from_expression(arg_expr)
                                        .unwrap_or(make_type(TypeKind::Error));
                                    mapping.insert(base_generic.name.clone(), arg_type);
                                }
                            }
                        }
                    }
                }
            }
        }
        mapping
    }

    fn report_class_member_not_found(
        &mut self,
        name: &str,
        prop_name: &str,
        context: &mut Context,
        span: Span,
    ) -> Type {
        let mut candidates: Vec<&str> = Vec::new();
        let mut collect_class_name = name;
        loop {
            let collect_def_opt = context
                .resolve_type_definition(collect_class_name)
                .or_else(|| self.global_type_definitions.get(collect_class_name));

            if let Some(TypeDefinition::Class(collect_def)) = collect_def_opt {
                candidates.extend(collect_def.fields.iter().map(|(n, _)| n.as_str()));
                candidates.extend(collect_def.methods.keys().map(|k| k.as_str()));

                if let Some(base_name) = &collect_def.base_class {
                    collect_class_name = base_name.as_str();
                    continue;
                }
            }
            break;
        }

        if let Some(suggestion) = find_best_match(prop_name, &candidates) {
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
        make_type(TypeKind::Error)
    }

    fn build_class_method_type(
        &mut self,
        method_info: &crate::type_checker::context::MethodInfo,
        mapping: &std::collections::HashMap<String, Type>,
    ) -> Type {
        let params: Vec<Parameter> = method_info
            .params
            .iter()
            .enumerate()
            .map(|(i, (pname, ty))| {
                let substituted_ty = if mapping.is_empty() {
                    ty.clone()
                } else {
                    self.substitute_type(ty, mapping)
                };
                Parameter {
                    name: pname.clone(),
                    typ: Box::new(self.create_type_expression(substituted_ty)),
                    guard: None,
                    default_value: None,
                    is_out: method_info.is_param_out(i),
                }
            })
            .collect();

        let return_type_expr = if matches!(method_info.return_type.kind, TypeKind::Void) {
            None
        } else {
            let substituted_ret = if mapping.is_empty() {
                method_info.return_type.clone()
            } else {
                self.substitute_type(&method_info.return_type, mapping)
            };
            Some(Box::new(self.create_type_expression(substituted_ret)))
        };

        make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: None,
            params,
            return_type: return_type_expr,
        })))
    }

    fn infer_member_class_trait_fallback(
        &mut self,
        name: &str,
        prop_name: &str,
        type_args: &Option<Vec<Expression>>,
    ) -> Option<Type> {
        let receiver_mapping = self.build_receiver_mapping(name, type_args);

        let mut search_class_name = Some(name.to_string());
        while let Some(class_name) = search_class_name.take() {
            let (traits, base_class) = match self.global_type_definitions.get(&class_name) {
                Some(TypeDefinition::Class(class_def)) => {
                    (class_def.traits.clone(), class_def.base_class.clone())
                }
                _ => break,
            };
            for trait_name in &traits {
                if let Some(ty) = self.search_trait_method(trait_name, prop_name, &receiver_mapping)
                {
                    return Some(ty);
                }
            }
            if let Some(base_name) = base_class {
                search_class_name = Some(base_name);
                continue;
            }
            break;
        }
        None
    }

    fn build_receiver_mapping(
        &mut self,
        name: &str,
        type_args: &Option<Vec<Expression>>,
    ) -> std::collections::HashMap<String, Type> {
        let mut m = std::collections::HashMap::new();
        if let Some(orig_generics) = self.global_type_definitions.get(name).and_then(|td| {
            if let TypeDefinition::Class(cd) = td {
                cd.generics.clone()
            } else {
                None
            }
        }) {
            if let Some(type_args) = type_args {
                for (gen, arg_expr) in orig_generics.iter().zip(type_args.iter()) {
                    let arg_type = self
                        .extract_type_from_expression(arg_expr)
                        .unwrap_or(make_type(TypeKind::Error));
                    m.insert(gen.name.clone(), arg_type);
                }
            }
        }
        m
    }

    fn search_trait_method(
        &mut self,
        trait_name: &str,
        prop_name: &str,
        receiver_mapping: &std::collections::HashMap<String, Type>,
    ) -> Option<Type> {
        let mut to_check: Vec<String> = vec![trait_name.to_string()];
        let mut visited = std::collections::HashSet::new();
        while let Some(t_name) = to_check.pop() {
            if !visited.insert(t_name.clone()) {
                continue;
            }
            let (method_opt, parents) = match self.global_type_definitions.get(&t_name) {
                Some(TypeDefinition::Trait(t_def)) => (
                    t_def.methods.get(prop_name).cloned(),
                    t_def.parent_traits.clone(),
                ),
                _ => continue,
            };
            if let Some(method_info) = method_opt {
                if !method_info.is_abstract {
                    return Some(self.build_method_type(&method_info, receiver_mapping));
                }
            }
            to_check.extend(parents);
        }
        None
    }

    fn build_method_type(
        &mut self,
        method_info: &crate::type_checker::context::MethodInfo,
        receiver_mapping: &std::collections::HashMap<String, Type>,
    ) -> Type {
        let substitute = |ty: &Type| -> Type {
            if receiver_mapping.is_empty() {
                ty.clone()
            } else {
                self.substitute_type(ty, receiver_mapping)
            }
        };

        let params: Vec<Parameter> = method_info
            .params
            .iter()
            .enumerate()
            .map(|(i, (pname, ty))| Parameter {
                name: pname.clone(),
                typ: Box::new(self.create_type_expression(substitute(ty))),
                guard: None,
                default_value: None,
                is_out: method_info.is_param_out(i),
            })
            .collect();

        let return_type_expr = if matches!(method_info.return_type.kind, TypeKind::Void) {
            None
        } else {
            Some(Box::new(self.create_type_expression(substitute(
                &method_info.return_type,
            ))))
        };

        make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: None,
            params,
            return_type: return_type_expr,
        })))
    }

    fn infer_member_trait(
        &mut self,
        name: &str,
        trait_def: &crate::type_checker::context::TraitDefinition,
        prop_name: &str,
        span: Span,
        _context: &mut Context,
    ) -> Type {
        let mut to_check: Vec<String> = vec![name.to_string()];
        let mut visited = std::collections::HashSet::new();
        while let Some(t_name) = to_check.pop() {
            if !visited.insert(t_name.clone()) {
                continue;
            }
            let (method_info, parent_traits) = if t_name == name {
                (
                    trait_def.methods.get(prop_name).cloned(),
                    trait_def.parent_traits.clone(),
                )
            } else {
                match self.global_type_definitions.get(&t_name) {
                    Some(TypeDefinition::Trait(d)) => {
                        (d.methods.get(prop_name).cloned(), d.parent_traits.clone())
                    }
                    _ => continue,
                }
            };
            if let Some(method_info) = method_info {
                let empty_map = std::collections::HashMap::new();
                return self.build_method_type(&method_info, &empty_map);
            }
            to_check.extend(parent_traits);
        }

        let all_methods = self.collect_trait_methods_for_access(name);
        if let Some(suggestion) = find_best_match(prop_name, &all_methods) {
            self.report_error_with_help(
                format!("Trait '{}' has no method '{}'", name, prop_name),
                span,
                format!("Did you mean '{}'?", suggestion),
            );
        } else {
            self.report_error(
                format!("Trait '{}' has no method '{}'", name, prop_name),
                span,
            );
        }
        make_type(TypeKind::Error)
    }

    fn collect_trait_methods_for_access(&mut self, name: &str) -> Vec<&str> {
        let mut methods: Vec<&str> = Vec::new();
        let mut all_to_check = vec![name];
        let mut all_visited = std::collections::HashSet::new();
        while let Some(t_name) = all_to_check.pop() {
            if !all_visited.insert(t_name) {
                continue;
            }
            if let Some(TypeDefinition::Trait(td)) = self.global_type_definitions.get(t_name) {
                methods.extend(td.methods.keys().map(|s| s.as_str()));
                all_to_check.extend(td.parent_traits.iter().map(|s| s.as_str()));
            }
        }
        methods
    }

    fn infer_member_enum(
        &mut self,
        enum_def: &crate::type_checker::context::EnumDefinition,
        name: &str,
        prop_name: &str,
        obj_type: &Type,
        span: Span,
        _context: &mut Context,
    ) -> Type {
        if let Some(method_info) = enum_def.methods.get(prop_name) {
            let type_args: Option<Vec<crate::ast::Expression>> =
                if let TypeKind::Custom(_, args) = &obj_type.kind {
                    args.clone()
                } else {
                    None
                };
            let mut mapping = HashMap::new();
            if let (Some(args), Some(generics)) = (&type_args, &enum_def.generics) {
                if generics.len() == args.len() {
                    for (param, arg_expr) in generics.iter().zip(args.iter()) {
                        let arg_type = self
                            .extract_type_from_expression(arg_expr)
                            .unwrap_or(make_type(TypeKind::Error));
                        mapping.insert(param.name.clone(), arg_type);
                    }
                }
            }

            let params: Vec<Parameter> = method_info
                .params
                .iter()
                .enumerate()
                .map(|(i, (pname, ty))| {
                    let substituted_ty = if mapping.is_empty() {
                        ty.clone()
                    } else {
                        self.substitute_type(ty, &mapping)
                    };
                    Parameter {
                        name: pname.clone(),
                        typ: Box::new(self.create_type_expression(substituted_ty)),
                        guard: None,
                        default_value: None,
                        is_out: method_info.is_param_out(i),
                    }
                })
                .collect();

            let return_type_expr = if matches!(method_info.return_type.kind, TypeKind::Void) {
                None
            } else {
                let substituted_ret = if mapping.is_empty() {
                    method_info.return_type.clone()
                } else {
                    self.substitute_type(&method_info.return_type, &mapping)
                };
                Some(Box::new(self.create_type_expression(substituted_ret)))
            };

            return make_type(TypeKind::Function(Box::new(FunctionTypeData {
                generics: None,
                params,
                return_type: return_type_expr,
            })));
        }
        self.report_error(
            format!("Enum '{}' has no method '{}'", name, prop_name),
            span,
        );
        make_type(TypeKind::Error)
    }

    fn infer_member_meta(
        &mut self,
        inner_type: &Type,
        prop_name: &str,
        span: Span,
        context: &mut Context,
    ) -> Type {
        if let TypeKind::Custom(name, _) = &inner_type.kind {
            let def_opt = self.resolve_visible_type(name, context).cloned();

            if let Some(TypeDefinition::Enum(def)) = &def_opt {
                if let Some(variant_types) = def.variants.get(prop_name) {
                    let type_args = if let TypeKind::Custom(_, args) = &inner_type.kind {
                        args.clone()
                    } else {
                        None
                    };

                    if variant_types.is_empty() {
                        return make_type(TypeKind::Custom(name.clone(), type_args));
                    }

                    let variant_types_clone = variant_types.clone();
                    return self.infer_member_enum_variant(
                        name,
                        &variant_types_clone,
                        def,
                        &type_args,
                    );
                }

                let candidates: Vec<&str> = def.variants.keys().map(|s| s.as_str()).collect();
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
                return make_type(TypeKind::Error);
            }

            self.report_error(
                format!("Type '{}' does not have static members", name),
                span,
            );
            return make_type(TypeKind::Error);
        }

        self.report_error(
            format!("Type '{}' does not have static members", inner_type),
            span,
        );
        make_type(TypeKind::Error)
    }

    fn infer_member_enum_variant(
        &mut self,
        name: &str,
        variant_types: &[Type],
        def: &crate::type_checker::context::EnumDefinition,
        type_args: &Option<Vec<Expression>>,
    ) -> Type {
        let substituted_variant_types =
            self.substitute_variant_types(variant_types, def, type_args);
        let params = self.build_variant_params(&substituted_variant_types);
        let (fn_generics, return_type_args) = self.build_variant_generics(def, type_args);

        make_type(TypeKind::Function(Box::new(FunctionTypeData {
            generics: fn_generics,
            params,
            return_type: Some(Box::new(self.create_type_expression(make_type(
                TypeKind::Custom(name.to_string(), return_type_args),
            )))),
        })))
    }

    fn substitute_variant_types(
        &mut self,
        variant_types: &[Type],
        def: &crate::type_checker::context::EnumDefinition,
        type_args: &Option<Vec<Expression>>,
    ) -> Vec<Type> {
        if let Some(generics) = &def.generics {
            if let Some(args) = type_args {
                if generics.len() == args.len() {
                    let mut mapping = HashMap::new();
                    for (param, arg_expr) in generics.iter().zip(args.iter()) {
                        let arg_type = self
                            .extract_type_from_expression(arg_expr)
                            .unwrap_or(make_type(TypeKind::Error));
                        mapping.insert(param.name.clone(), arg_type);
                    }
                    return variant_types
                        .iter()
                        .map(|t| self.substitute_type(t, &mapping))
                        .collect();
                }
            }
        }
        variant_types.to_vec()
    }

    fn build_variant_params(&self, substituted_variant_types: &[Type]) -> Vec<Parameter> {
        substituted_variant_types
            .iter()
            .enumerate()
            .map(|(i, t)| Parameter {
                name: format!("arg{}", i),
                typ: Box::new(self.create_type_expression(t.clone())),
                guard: None,
                default_value: None,
                is_out: false,
            })
            .collect()
    }

    fn build_variant_generics(
        &self,
        def: &crate::type_checker::context::EnumDefinition,
        type_args: &Option<Vec<Expression>>,
    ) -> (Option<Vec<Expression>>, Option<Vec<Expression>>) {
        if type_args.is_none() {
            if let Some(generics) = &def.generics {
                let gen_exprs: Vec<Expression> = generics
                    .iter()
                    .map(|g| {
                        let constraint_expr = g
                            .constraint
                            .as_ref()
                            .map(|c| Box::new(self.create_type_expression(c.clone())));
                        ast_factory::generic_type_with_kind(&g.name, constraint_expr, g.kind)
                    })
                    .collect();
                let ret_args: Vec<Expression> = generics
                    .iter()
                    .map(|g| {
                        self.create_type_expression(make_type(TypeKind::Generic(
                            g.name.clone(),
                            g.constraint.clone().map(Box::new),
                            g.kind,
                        )))
                    })
                    .collect();
                return (Some(gen_exprs), Some(ret_args));
            }
        }
        (None, type_args.clone())
    }
}
