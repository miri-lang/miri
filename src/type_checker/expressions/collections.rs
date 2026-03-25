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

use crate::ast::factory::make_type;
use crate::ast::types::{Type, TypeKind};
use crate::ast::*;
use crate::type_checker::context::Context;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    pub(crate) fn infer_list(&mut self, elements: &[Expression], context: &mut Context) -> Type {
        if elements.is_empty() {
            return make_type(TypeKind::Custom(
                "List".to_string(),
                Some(vec![self.create_type_expression(make_type(TypeKind::Void))]),
            ));
        }

        let first_type = self.infer_expression(&elements[0], context);
        let mut has_error = false;

        for element in &elements[1..] {
            let element_type = self.infer_expression(element, context);
            if !self.are_compatible(&first_type, &element_type, context) {
                self.report_error(
                    "Array elements must have the same type".to_string(),
                    element.span,
                );
                has_error = true;
            }
        }

        if has_error {
            return make_type(TypeKind::Error);
        }

        make_type(TypeKind::Custom(
            "List".to_string(),
            Some(vec![self.create_type_expression(first_type)]),
        ))
    }

    /// Infers the type of an array literal expression (`[1, 2, 3]`).
    ///
    /// All elements must have the same type. Returns `Array(element_type, size)`.
    pub(crate) fn infer_array(
        &mut self,
        elements: &[Expression],
        size: &Expression,
        context: &mut Context,
    ) -> Type {
        if elements.is_empty() {
            let inner_type_expr = self.create_type_expression(make_type(TypeKind::Void));
            return make_type(TypeKind::Custom(
                "Array".to_string(),
                Some(vec![inner_type_expr, size.clone()]),
            ));
        }

        let first_type = self.infer_expression(&elements[0], context);
        let mut has_error = false;

        for element in &elements[1..] {
            let element_type = self.infer_expression(element, context);
            if !self.are_compatible(&first_type, &element_type, context) {
                self.report_error(
                    "Array elements must have the same type".to_string(),
                    element.span,
                );
                has_error = true;
            }
        }

        if has_error {
            return make_type(TypeKind::Error);
        }

        make_type(TypeKind::Custom(
            "Array".to_string(),
            Some(vec![self.create_type_expression(first_type), size.clone()]),
        ))
    }

    pub(crate) fn infer_map(
        &mut self,
        entries: &[(Expression, Expression)],
        context: &mut Context,
    ) -> Type {
        if entries.is_empty() {
            return make_type(TypeKind::Custom(
                "Map".to_string(),
                Some(vec![
                    self.create_type_expression(make_type(TypeKind::Void)),
                    self.create_type_expression(make_type(TypeKind::Void)),
                ]),
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
                self.report_error("Map keys must have the same type".to_string(), key.span);
                has_error = true;
            }
            if !self.are_compatible(&val_type, &v_type, context) {
                self.report_error("Map values must have the same type".to_string(), val.span);
                has_error = true;
            }
        }

        if has_error {
            return make_type(TypeKind::Error);
        }

        make_type(TypeKind::Custom(
            "Map".to_string(),
            Some(vec![
                self.create_type_expression(key_type),
                self.create_type_expression(val_type),
            ]),
        ))
    }

    pub(crate) fn infer_set(&mut self, elements: &[Expression], context: &mut Context) -> Type {
        if elements.is_empty() {
            return make_type(TypeKind::Custom(
                "Set".to_string(),
                Some(vec![self.create_type_expression(make_type(TypeKind::Void))]),
            ));
        }

        let first_type = self.infer_expression(&elements[0], context);
        let mut has_error = false;

        for element in &elements[1..] {
            let element_type = self.infer_expression(element, context);
            if !self.are_compatible(&first_type, &element_type, context) {
                self.report_error(
                    "Set elements must have the same type".to_string(),
                    element.span,
                );
                has_error = true;
            }
        }

        if has_error {
            return make_type(TypeKind::Error);
        }

        if let TypeKind::Option(_) = first_type.kind {
            self.report_error(
                "Set elements cannot be optional".to_string(),
                elements[0].span,
            );
        }

        make_type(TypeKind::Custom(
            "Set".to_string(),
            Some(vec![self.create_type_expression(first_type)]),
        ))
    }

    pub(crate) fn infer_tuple(&mut self, elements: &[Expression], context: &mut Context) -> Type {
        let mut element_types = Vec::with_capacity(elements.len());
        for element in elements {
            let ty = self.infer_expression(element, context);
            element_types.push(self.create_type_expression(ty));
        }
        make_type(TypeKind::Tuple(element_types))
    }
}
