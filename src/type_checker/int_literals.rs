// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The width an integer literal takes from the context that consumes it.
//!
//! A decimal literal is written without a width — Miri has neither literal
//! suffixes nor a type annotation on every binding — so inference types every
//! integer literal as the default `int` and the real width is decided by what
//! the literal is written into: a declared binding, a parameter, a constructor
//! field, a collection's element. Recording that width on the literal is what
//! lets a value the default `int` cannot hold reach its slot whole, and what
//! lets a negation happen at the slot's width rather than at 64 bits.
//!
//! This mirrors the narrowing the float widths get, and for the same reason:
//! only the consuming context knows the width, and it learns the literal's own
//! type before it can compare the two.

use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind};
use crate::ast::*;
use crate::type_checker::TypeChecker;

/// Whether a type is one of the integer widths a literal can be recorded at.
pub(crate) fn is_integer_width(kind: &TypeKind) -> bool {
    matches!(
        kind,
        TypeKind::Int
            | TypeKind::I8
            | TypeKind::I16
            | TypeKind::I32
            | TypeKind::I64
            | TypeKind::I128
            | TypeKind::U8
            | TypeKind::U16
            | TypeKind::U32
            | TypeKind::U64
            | TypeKind::U128
    )
}

impl TypeChecker {
    /// Records `expected`'s integer width on every literal inside `expr` that
    /// was typed as the default `int`, returning the type `expr` then has.
    ///
    /// `None` when `expr` holds nothing a width applies to, which leaves the
    /// inferred type alone. The shape walked is the same one a float literal
    /// narrows through: the literal itself, a sign in front of it, a collection
    /// literal's elements, and a collection constructor wrapping one.
    pub(crate) fn widen_int_literals(
        &mut self,
        expr: &Expression,
        expected: &Type,
        inferred: &Type,
    ) -> Option<Type> {
        match &expr.node {
            ExpressionKind::Literal(Literal::Integer(_))
                if is_integer_width(&expected.kind) && is_integer_width(&inferred.kind) =>
            {
                let width = Type::new(expected.kind.clone(), expected.span);
                self.record_int_literal_width(expr, &width);
                Some(width)
            }
            // A sign is not itself a width: the literal under it takes the
            // width, and the negation is then performed at that width instead of
            // at the default `int`'s.
            ExpressionKind::Unary(UnaryOp::Negate | UnaryOp::Plus, operand) => {
                let widened = self.widen_int_literals(operand, expected, inferred)?;
                self.record_int_literal_width(expr, &widened);
                Some(widened)
            }
            // A collection constructor wrapping a literal (`List<i128>([1, 2])`)
            // widens through to that literal: the constructor only chooses the
            // collection, the elements are still written in the source.
            ExpressionKind::Call(_, args)
                if args.len() == 1
                    && matches!(
                        args[0].node,
                        ExpressionKind::List(_) | ExpressionKind::Array(_, _)
                    )
                    && self.int_sequence_element_type(expected).is_some()
                    && self.int_sequence_element_type(inferred).is_some() =>
            {
                let literal_inferred = self.type_table.types.get(&args[0].id)?.clone();
                let widened_literal =
                    self.widen_int_literals(&args[0], expected, &literal_inferred)?;
                let element = self.int_sequence_element_type(&widened_literal)?;
                let widened = self.with_int_element_type(inferred, &element)?;
                self.record_int_literal_width(expr, &widened);
                Some(widened)
            }
            ExpressionKind::List(elements) | ExpressionKind::Array(elements, _) => {
                let expected_element = self.int_sequence_element_type(expected)?;
                let inferred_element = self.int_sequence_element_type(inferred)?;
                let mut widened_element = None;
                for element in elements {
                    widened_element =
                        self.widen_int_literals(element, &expected_element, &inferred_element);
                    widened_element.as_ref()?;
                }
                let widened = self.with_int_element_type(inferred, &widened_element?)?;
                self.record_int_literal_width(expr, &widened);
                Some(widened)
            }
            _ => None,
        }
    }

    /// Records `element` as the width of the integer literals in a sequence
    /// argument whose element type comes from a constructor's type argument
    /// (`List<i128>([1, 2])`) rather than from a declared parameter.
    ///
    /// Without this the elements keep the default `int` they were inferred as,
    /// and a collection whose slots are wider than that is handed values filling
    /// only part of each one.
    pub(crate) fn widen_sequence_argument_elements(
        &mut self,
        arg_expr: &Expression,
        arg_type: &Type,
        element: &Type,
    ) {
        if !is_integer_width(&element.kind) {
            return;
        }
        let Some(expected) = self.with_int_element_type(arg_type, element) else {
            return;
        };
        self.widen_int_literals(arg_expr, &expected, arg_type);
    }

    /// Records `ty` as the type of `expr`, replacing what inference recorded.
    fn record_int_literal_width(&mut self, expr: &Expression, ty: &Type) {
        self.type_table
            .types
            .insert(expr.id, Type::new(ty.kind.clone(), expr.span));
    }

    /// The element type of a built-in sequence type (`List`, `Array`, `Set`), or
    /// `None` for anything a collection literal cannot widen into. A `Map`
    /// literal is not a sequence of elements, so it widens nowhere here.
    ///
    /// Both spellings are accepted: the canonical `TypeKind::List`/`Array`/`Set`
    /// variants a field or parameter declaration carries, and the normalized
    /// `Custom("List", [..])` form inference produces.
    fn int_sequence_element_type(&self, collection: &Type) -> Option<Type> {
        match &collection.kind {
            TypeKind::List(inner) | TypeKind::Set(inner) | TypeKind::Array(inner, _) => {
                self.extract_type_from_expression(inner).ok()
            }
            TypeKind::Custom(name, Some(args)) => {
                match BuiltinCollectionKind::from_name(name.as_str())? {
                    BuiltinCollectionKind::List
                    | BuiltinCollectionKind::Array
                    | BuiltinCollectionKind::Set => {
                        self.extract_type_from_expression(args.first()?).ok()
                    }
                    BuiltinCollectionKind::Map => None,
                }
            }
            _ => None,
        }
    }

    /// `collection` with its element type replaced, every other part of the type
    /// (an array's size) left as it was.
    fn with_int_element_type(&mut self, collection: &Type, element: &Type) -> Option<Type> {
        let element_expr = self.create_type_expression(element.clone());
        let kind = match &collection.kind {
            TypeKind::List(_) => TypeKind::List(Box::new(element_expr)),
            TypeKind::Set(_) => TypeKind::Set(Box::new(element_expr)),
            TypeKind::Array(_, size) => {
                TypeKind::Array(Box::new(element_expr), Box::new((**size).clone()))
            }
            TypeKind::Custom(name, Some(args)) => {
                let mut widened_args = args.clone();
                *widened_args.first_mut()? = element_expr;
                TypeKind::Custom(name.clone(), Some(widened_args))
            }
            _ => return None,
        };
        Some(Type::new(kind, collection.span))
    }
}
