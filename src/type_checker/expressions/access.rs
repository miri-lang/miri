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
use crate::ast::types::{Type, TypeKind};
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

                // Compile-time bounds check for slicing with constant range bounds
                let check_slice_bounds = |tc: &mut TypeChecker,
                                          array_size: Option<i128>,
                                          index: &Expression,
                                          span: Span| {
                    if let ExpressionKind::Range(start, end, _range_type) = &index.node {
                        if let Some(size) = array_size {
                            if let Some(start_val) =
                                Self::try_eval_const_int_with_context(start, context)
                            {
                                if start_val < 0 {
                                    tc.report_error(
                                        "Slice start index must be a non-negative integer"
                                            .to_string(),
                                        start.span,
                                    );
                                } else if start_val > size {
                                    tc.report_error(
                                        format!(
                                            "Slice start index out of bounds: index {} but array has {} elements",
                                            start_val, size
                                        ),
                                        start.span,
                                    );
                                }
                            }
                            if let Some(end_expr) = end {
                                if let Some(end_val) =
                                    Self::try_eval_const_int_with_context(end_expr, context)
                                {
                                    if end_val < 0 {
                                        tc.report_error(
                                            "Slice end index must be a non-negative integer"
                                                .to_string(),
                                            end_expr.span,
                                        );
                                    } else if end_val > size {
                                        tc.report_error(
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

                        // Check start <= end when both are constant
                        if let Some(end_expr) = end {
                            if let (Some(s), Some(e)) = (
                                Self::try_eval_const_int_with_context(start, context),
                                Self::try_eval_const_int_with_context(end_expr, context),
                            ) {
                                if s > e {
                                    tc.report_error(
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
                };

                match obj_type.kind {
                    TypeKind::String => return make_type(TypeKind::String),
                    TypeKind::List(inner) => return make_type(TypeKind::List(inner)),
                    TypeKind::Array(inner, size_expr) => {
                        let array_size = Self::try_eval_const_int(&size_expr);
                        check_slice_bounds(self, array_size, index, span);
                        // Compute slice size from range bounds
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
                                let size = Box::new(crate::ast::factory::int_literal_expression(
                                    slice_size,
                                ));
                                return make_type(TypeKind::Array(inner, size));
                            }
                        }
                        // Fallback: unknown slice size, return original
                        return make_type(TypeKind::Array(inner, size_expr));
                    }
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
            TypeKind::Array(inner_type_expr, size_expr) => {
                if !matches!(index_type.kind, TypeKind::Int) {
                    self.report_error("Array index must be an integer".to_string(), index.span);
                    return make_type(TypeKind::Error);
                }
                // Compile-time bounds check using constant evaluation
                if let Some(idx_val) = Self::try_eval_const_int_with_context(index, context) {
                    if idx_val < 0 {
                        self.report_error(
                            "Array index must be a non-negative integer".to_string(),
                            index.span,
                        );
                        return make_type(TypeKind::Error);
                    }
                    if let Some(size_val) = Self::try_eval_const_int(&size_expr) {
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
                self.resolve_type_expression(&inner_type_expr, context)
            }
            TypeKind::List(inner_type_expr) => {
                if !matches!(index_type.kind, TypeKind::Int) {
                    self.report_error("List index must be an integer".to_string(), index.span);
                    return make_type(TypeKind::Error);
                }
                self.resolve_type_expression(&inner_type_expr, context)
            }
            TypeKind::Custom(name, args)
                if name == "Array" || name == "List" || name == "Tuple" =>
            {
                if !matches!(index_type.kind, TypeKind::Int) {
                    self.report_error(format!("{} index must be an integer", name), index.span);
                    return make_type(TypeKind::Error);
                }
                if let Some(args) = args {
                    if let Some(inner_type_expr) = args.first() {
                        return self.resolve_type_expression(inner_type_expr, context);
                    }
                } else {
                    // Inside the class definition itself, args is None.
                    // We can look up the generic parameter 'T' from the context.
                    if let Some(TypeDefinition::Generic(g)) = context.resolve_type_definition("T") {
                        return make_type(TypeKind::Generic(
                            g.name.clone(),
                            g.constraint.clone().map(Box::new),
                            g.kind.clone(),
                        ));
                    } else {
                        return make_type(TypeKind::Generic(
                            "T".to_string(),
                            None,
                            TypeDeclarationKind::None,
                        ));
                    }
                }
                make_type(TypeKind::Error)
            }
            TypeKind::Map(key_type_expr, val_type_expr) => {
                let key_type = self.resolve_type_expression(&key_type_expr, context);
                if !self.are_compatible(&key_type, &index_type, context) {
                    self.report_error("Invalid map key type".to_string(), index.span);
                    return make_type(TypeKind::Error);
                }
                self.resolve_type_expression(&val_type_expr, context)
            }
            TypeKind::Custom(name, args) if name == "Map" => {
                // Inside the Map class definition, self is Custom("Map", None).
                // The key type is generic K, the value type is generic V.
                if let Some(args) = args {
                    if args.len() >= 2 {
                        let key_type = self.resolve_type_expression(&args[0], context);
                        if !self.are_compatible(&key_type, &index_type, context) {
                            self.report_error("Invalid map key type".to_string(), index.span);
                            return make_type(TypeKind::Error);
                        }
                        return self.resolve_type_expression(&args[1], context);
                    }
                }
                // Inside the class body, args is None — resolve generics from context.
                if let Some(TypeDefinition::Generic(g)) = context.resolve_type_definition("V") {
                    return make_type(TypeKind::Generic(
                        g.name.clone(),
                        g.constraint.clone().map(Box::new),
                        g.kind.clone(),
                    ));
                }
                make_type(TypeKind::Generic(
                    "V".to_string(),
                    None,
                    TypeDeclarationKind::None,
                ))
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
                        self.report_error("Tuple index must be an integer".to_string(), index.span);
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
                        let idx = val.to_usize();
                        if idx >= element_type_exprs.len() {
                            self.report_error("Tuple index out of bounds".to_string(), span);
                            return make_type(TypeKind::Error);
                        }
                    }

                    self.resolve_type_expression(&element_type_exprs[0], context)
                } else {
                    // For heterogeneous tuple, index must be a compile-time integer literal
                    if let ExpressionKind::Literal(Literal::Integer(val)) = &index.node {
                        let idx = val.to_usize();

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
                            index.span,
                        );
                        make_type(TypeKind::Error)
                    }
                }
            }
            TypeKind::String => {
                if !matches!(index_type.kind, TypeKind::Int) {
                    self.report_error("String index must be an integer".to_string(), index.span);
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
        let obj_type = self.infer_expression(obj, context);

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

        // Try to resolve the type definition for the object's type
        let (type_name, type_args) = match &obj_type.kind {
            TypeKind::String => (Some("String".to_string()), None),
            TypeKind::List(inner_expr) => {
                let inner_ty = self.resolve_type_expression(inner_expr, context);
                (
                    Some("List".to_string()),
                    Some(vec![self.create_type_expression(inner_ty)]),
                )
            }
            TypeKind::Map(key_expr, val_expr) => {
                let key_ty = self.resolve_type_expression(key_expr, context);
                let val_ty = self.resolve_type_expression(val_expr, context);
                (
                    Some("Map".to_string()),
                    Some(vec![
                        self.create_type_expression(key_ty),
                        self.create_type_expression(val_ty),
                    ]),
                )
            }
            TypeKind::Set(inner_expr) => {
                let inner_ty = self.resolve_type_expression(inner_expr, context);
                (
                    Some("Set".to_string()),
                    Some(vec![self.create_type_expression(inner_ty)]),
                )
            }
            TypeKind::Tuple(element_type_exprs) => {
                // For homogeneous tuples, resolve to "Tuple" class with element type
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
            TypeKind::Array(inner_expr, size_expr) => {
                let inner_ty = self.resolve_type_expression(inner_expr, context);
                (
                    Some("Array".to_string()),
                    Some(vec![
                        self.create_type_expression(inner_ty),
                        *size_expr.clone(),
                    ]),
                )
            }
            TypeKind::Result(ok_type, _) => {
                if prop_name == "unwrap" {
                    let t = self.resolve_type_expression(ok_type, context);
                    return make_type(TypeKind::Function(Box::new(FunctionTypeData {
                        generics: None,
                        params: vec![],
                        return_type: Some(Box::new(ast_factory::type_expr_non_null(t))),
                    })));
                } else if prop_name == "is_ok" || prop_name == "is_err" {
                    return make_type(TypeKind::Function(Box::new(FunctionTypeData {
                        generics: None,
                        params: vec![],
                        return_type: Some(Box::new(ast_factory::type_expr_non_null(make_type(
                            TypeKind::Boolean,
                        )))),
                    })));
                }
                (None, None)
            }
            TypeKind::Option(_) => (None, None),
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

                return ast_factory::make_type(TypeKind::Function(Box::new(FunctionTypeData {
                    generics: None,
                    params: vec![
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
                    return_type: Some(Box::new(ast_factory::type_expr_non_null(future_void_type))),
                })));
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
                    // Substitute generic parameters if present
                    let mut mapping = std::collections::HashMap::new();
                    if let Some(generics) = &search_class_def.generics {
                        if let Some(type_args) = &type_args {
                            if generics.len() == type_args.len() {
                                for (param, arg_expr) in generics.iter().zip(type_args.iter()) {
                                    let arg_type = self
                                        .extract_type_from_expression(arg_expr)
                                        .unwrap_or(make_type(TypeKind::Error));
                                    mapping.insert(param.name.clone(), arg_type);
                                }
                            }
                        }
                    }

                    // Check fields in current class
                    if let Some((_, field_info)) =
                        search_class_def.fields.iter().find(|(n, _)| n == prop_name)
                    {
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

                        if mapping.is_empty() {
                            return field_info.ty.clone();
                        } else {
                            return self.substitute_type(&field_info.ty, &mapping);
                        }
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
                            .map(|(name, ty)| {
                                let substituted_ty = if mapping.is_empty() {
                                    ty.clone()
                                } else {
                                    self.substitute_type(ty, &mapping)
                                };
                                Parameter {
                                    name: name.clone(),
                                    typ: Box::new(self.create_type_expression(substituted_ty)),
                                    guard: None,
                                    default_value: None,
                                }
                            })
                            .collect();

                        let return_type_expr =
                            if matches!(method_info.return_type.kind, TypeKind::Void) {
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
                        candidates.extend(collect_def.fields.iter().map(|(n, _)| n.clone()));
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
            } else if def_opt.is_none() {
                let module_to_import = match name.as_str() {
                    "Array" => Some("system.collections.array"),
                    "List" => Some("system.collections.list"),
                    "Map" => Some("system.collections.map"),
                    "Set" => Some("system.collections.set"),
                    _ => None,
                };

                if let Some(module) = module_to_import {
                    self.report_error_with_help(
                        format!("Type '{}' does not have members", obj_type),
                        span,
                        format!("Consider importing '{}' to use {} methods", module, name),
                    );
                } else if name == "String" {
                    if prop_name == "length" {
                        return make_type(TypeKind::Int);
                    } else {
                        self.report_error(
                            format!("Type 'String' has no field '{}'", prop_name),
                            span,
                        );
                    }
                } else {
                    self.report_error(format!("Type '{}' does not have members", obj_type), span);
                }
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
                                let mut substituted_variant_types =
                                    Vec::with_capacity(variant_types.len());
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
                                make_type(TypeKind::Function(Box::new(FunctionTypeData {
                                    generics: None,
                                    params,
                                    return_type: Some(Box::new(self.create_type_expression(
                                        make_type(TypeKind::Custom(name.clone(), type_args)),
                                    ))),
                                })))
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
}
