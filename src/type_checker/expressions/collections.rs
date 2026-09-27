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
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::ast::*;
use crate::diagnostics::DiagnosticCode;
use crate::type_checker::context::Context;
use crate::type_checker::TypeChecker;

impl TypeChecker {
    pub(crate) fn infer_list(&mut self, elements: &[Expression], context: &mut Context) -> Type {
        if elements.is_empty() {
            return make_type(TypeKind::Custom(
                BuiltinCollectionKind::List.name().to_string(),
                Some(vec![self.create_type_expression(make_type(TypeKind::Void))]),
            ));
        }

        let first_type = self.infer_expression(&elements[0], context);
        let mut element_type = first_type.clone();
        let mut has_error = false;

        for element in &elements[1..] {
            let next_type = self.infer_expression(element, context);
            if !self.are_compatible(&first_type, &next_type, context) {
                self.report_error(
                    DiagnosticCode::TypCollectionElementType,
                    "Array elements must have the same type".to_string(),
                    element.span,
                );
                has_error = true;
            }
            element_type = fill_open_arguments(&element_type, &next_type);
        }

        if has_error {
            return make_type(TypeKind::Error);
        }
        for element in elements {
            self.record_joined_type(element, &element_type);
        }

        make_type(TypeKind::Custom(
            BuiltinCollectionKind::List.name().to_string(),
            Some(vec![self.create_type_expression(element_type)]),
        ))
    }

    /// Record `joined` as the type of `expr` — one branch of a conditional or
    /// match, or one element of a literal — when it left open an argument a
    /// sibling bound.
    ///
    /// The value is built at the type recorded for it, and the expression
    /// holding it reads it at the joined type: an `E.L(s)` built as an
    /// `E<String, B>` beside an `E<String, i128>` has to be laid out at the
    /// wider payload slots too, or a read through the joined type lands at the
    /// wrong offset.
    pub(crate) fn record_joined_type(&mut self, expr: &Expression, joined: &Type) {
        let Some(recorded) = self.get_type(expr.id) else {
            return;
        };
        let refined = fill_open_arguments(recorded, joined);
        if refined != *recorded {
            self.type_table.types.insert(expr.id, refined);
        }
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
                BuiltinCollectionKind::Array.name().to_string(),
                Some(vec![inner_type_expr, size.clone()]),
            ));
        }

        let first_type = self.infer_expression(&elements[0], context);
        let mut element_type = first_type.clone();
        let mut has_error = false;

        for element in &elements[1..] {
            let next_type = self.infer_expression(element, context);
            if !self.are_compatible(&first_type, &next_type, context) {
                self.report_error(
                    DiagnosticCode::TypCollectionElementType,
                    "Array elements must have the same type".to_string(),
                    element.span,
                );
                has_error = true;
            }
            element_type = fill_open_arguments(&element_type, &next_type);
        }

        if has_error {
            return make_type(TypeKind::Error);
        }
        for element in elements {
            self.record_joined_type(element, &element_type);
        }

        make_type(TypeKind::Custom(
            BuiltinCollectionKind::Array.name().to_string(),
            Some(vec![
                self.create_type_expression(element_type),
                size.clone(),
            ]),
        ))
    }

    pub(crate) fn infer_map(
        &mut self,
        entries: &[(Expression, Expression)],
        context: &mut Context,
    ) -> Type {
        if entries.is_empty() {
            return make_type(TypeKind::Custom(
                BuiltinCollectionKind::Map.name().to_string(),
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
                self.report_error(
                    DiagnosticCode::TypCollectionElementType,
                    "Map keys must have the same type".to_string(),
                    key.span,
                );
                has_error = true;
            }
            if !self.are_compatible(&val_type, &v_type, context) {
                self.report_error(
                    DiagnosticCode::TypCollectionElementType,
                    "Map values must have the same type".to_string(),
                    val.span,
                );
                has_error = true;
            }
        }

        if has_error {
            return make_type(TypeKind::Error);
        }

        make_type(TypeKind::Custom(
            BuiltinCollectionKind::Map.name().to_string(),
            Some(vec![
                self.create_type_expression(key_type),
                self.create_type_expression(val_type),
            ]),
        ))
    }

    pub(crate) fn infer_set(&mut self, elements: &[Expression], context: &mut Context) -> Type {
        if elements.is_empty() {
            return make_type(TypeKind::Custom(
                BuiltinCollectionKind::Set.name().to_string(),
                Some(vec![self.create_type_expression(make_type(TypeKind::Void))]),
            ));
        }

        let first_type = self.infer_expression(&elements[0], context);
        let mut has_error = false;

        for element in &elements[1..] {
            let element_type = self.infer_expression(element, context);
            if !self.are_compatible(&first_type, &element_type, context) {
                self.report_error(
                    DiagnosticCode::TypCollectionElementType,
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
                DiagnosticCode::TypCollectionElementType,
                "Set elements cannot be optional".to_string(),
                elements[0].span,
            );
        }

        make_type(TypeKind::Custom(
            BuiltinCollectionKind::Set.name().to_string(),
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

/// `known` with each type argument it leaves open filled from `other`, the
/// type of a later element of the same literal.
///
/// A variant constructor fixes only the arguments its payload names:
/// `Result.Err(s)` is a `Result<T, String>` and `Result.Ok(s)` a
/// `Result<String, E>`. A literal holding both holds `Result<String, String>`,
/// and every layer that releases or compares its elements must see that type,
/// not whichever argument the first element happened to leave open.
pub(crate) fn fill_open_arguments(known: &Type, other: &Type) -> Type {
    if matches!(known.kind, TypeKind::Generic(..)) {
        return if matches!(other.kind, TypeKind::Generic(..)) {
            known.clone()
        } else {
            other.clone()
        };
    }
    let kind = match (&known.kind, &other.kind) {
        (TypeKind::Custom(name, Some(args)), TypeKind::Custom(other_name, Some(other_args)))
            if name == other_name && args.len() == other_args.len() =>
        {
            TypeKind::Custom(name.clone(), Some(fill_open_expressions(args, other_args)))
        }
        (TypeKind::Option(inner), TypeKind::Option(other_inner)) => {
            TypeKind::Option(Box::new(fill_open_arguments(inner, other_inner)))
        }
        (TypeKind::Tuple(parts), TypeKind::Tuple(other_parts))
            if parts.len() == other_parts.len() =>
        {
            TypeKind::Tuple(fill_open_expressions(parts, other_parts))
        }
        (TypeKind::List(element), TypeKind::List(other_element)) => {
            TypeKind::List(Box::new(fill_open_expression(element, other_element)))
        }
        (TypeKind::Set(element), TypeKind::Set(other_element)) => {
            TypeKind::Set(Box::new(fill_open_expression(element, other_element)))
        }
        (TypeKind::Map(key, value), TypeKind::Map(other_key, other_value)) => TypeKind::Map(
            Box::new(fill_open_expression(key, other_key)),
            Box::new(fill_open_expression(value, other_value)),
        ),
        (TypeKind::Result(ok, err), TypeKind::Result(other_ok, other_err)) => TypeKind::Result(
            Box::new(fill_open_expression(ok, other_ok)),
            Box::new(fill_open_expression(err, other_err)),
        ),
        // Every other pairing either names no argument to fill or is two
        // different types, which the element compatibility check reports.
        (known_kind, _) => known_kind.clone(),
    };
    Type::new(kind, known.span)
}

fn fill_open_expressions(known: &[Expression], other: &[Expression]) -> Vec<Expression> {
    known
        .iter()
        .zip(other)
        .map(|(known, other)| fill_open_expression(known, other))
        .collect()
}

/// A type argument written as an expression, filled like
/// [`fill_open_arguments`]; a value argument (an array's size) is kept.
fn fill_open_expression(known: &Expression, other: &Expression) -> Expression {
    match (&known.node, &other.node) {
        (ExpressionKind::Type(known_ty, nullable), ExpressionKind::Type(other_ty, _)) => {
            let mut filled = known.clone();
            filled.node =
                ExpressionKind::Type(Box::new(fill_open_arguments(known_ty, other_ty)), *nullable);
            filled
        }
        _ => known.clone(),
    }
}
