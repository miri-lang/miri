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
            self.record_joined_type(element, &element_type, context);
        }

        make_type(TypeKind::Custom(
            BuiltinCollectionKind::List.name().to_string(),
            Some(vec![self.create_type_expression(element_type)]),
        ))
    }

    /// Whether a value of `actual`, written as `expr`, may be stored where
    /// `expected` is declared — and, when it may, settle it at `expected`.
    ///
    /// This is the one check every typed location runs its value through: a
    /// call's argument against its parameter, a constructor's against its
    /// field, a returned value against the return type, an initializer against
    /// its declared type, an assigned value against its target.
    ///
    /// A value a variant constructor builds binds only the arguments its
    /// payload names — `E.L(s)` is an `E<String, B>` — and is laid out without
    /// the rest. Stored where an `E<String, i128>` is declared, it has to be
    /// built at that type's wider payload slots, or every read and release
    /// through the location lands at the wrong offset. So the value, and each
    /// branch or element it is made of, is recorded at the expected type.
    pub(crate) fn accepts_value_at(
        &mut self,
        expected: &Type,
        actual: &Type,
        expr: Option<&Expression>,
        context: &mut Context,
    ) -> bool {
        if !self.are_compatible(expected, actual, context) {
            return false;
        }
        if let Some(expr) = expr {
            let mut built_elsewhere = Vec::new();
            self.settle_at_expected(expr, expected, context, &mut built_elsewhere);
            for (read, read_type) in built_elsewhere {
                let unbound = self
                    .inference_slots_bound_by(&read_type, expected, context)
                    .join("`, `");
                self.report_error(
                    DiagnosticCode::TypTypeInference,
                    format!(
                        "Cannot use a value of type {read_type} where {expected} is expected: \
                         its type leaves `{unbound}` unbound, so it was built at a layout that \
                         does not hold what {expected} does. Declare the type where the value \
                         is first written, with `{unbound}` filled in"
                    ),
                    read.span,
                );
            }
        }
        true
    }

    /// Record `expr`, and every branch or literal element it is made of, at
    /// `expected` wherever it left open an argument `expected` binds.
    ///
    /// Only an expression that builds its value — a variant constructor, a
    /// literal, a built-in collection built from one — is recorded again: its
    /// value is laid out at the recorded type. One that reads a value built
    /// elsewhere (a binding, a field, a call's result) keeps its type, which is
    /// the layout that value already has.
    fn settle_at_expected<'e>(
        &mut self,
        expr: &'e Expression,
        expected: &Type,
        context: &Context,
        built_elsewhere: &mut Vec<(&'e Expression, Type)>,
    ) {
        // A value stored where an optional is declared is wrapped on the way
        // (`let o E<String, i128>? = E.L(s)`): it is built at the payload type.
        if let TypeKind::Option(inner) = &expected.kind {
            let is_wrapped = self
                .get_type(expr.id)
                .is_some_and(|actual| !matches!(actual.kind, TypeKind::Option(_)));
            if is_wrapped {
                return self.settle_at_expected(expr, inner, context, built_elsewhere);
            }
        }
        match &expr.node {
            ExpressionKind::Tuple(elements) => {
                self.record_joined_type(expr, expected, context);
                let TypeKind::Tuple(element_types) = &expected.kind else {
                    return;
                };
                for (value, element_type) in elements.iter().zip(element_types) {
                    if let Ok(element_type) = self.extract_type_from_expression(element_type) {
                        self.settle_at_expected(value, &element_type, context, built_elsewhere);
                    }
                }
            }
            ExpressionKind::List(elements)
            | ExpressionKind::Array(elements, _)
            | ExpressionKind::Set(elements) => {
                self.record_joined_type(expr, expected, context);
                let Some(element) = self.literal_element_type(expected) else {
                    return;
                };
                for value in elements {
                    self.settle_at_expected(value, &element, context, built_elsewhere);
                }
            }
            ExpressionKind::Conditional(then_expr, _, else_expr, _) => {
                self.record_joined_type(expr, expected, context);
                self.settle_at_expected(then_expr, expected, context, built_elsewhere);
                if let Some(else_expr) = else_expr {
                    self.settle_at_expected(else_expr, expected, context, built_elsewhere);
                }
            }
            ExpressionKind::Match(_, branches) => {
                self.record_joined_type(expr, expected, context);
                for branch in branches {
                    if let crate::ast::statement::StatementKind::Expression(value) =
                        &branch.body.node
                    {
                        self.settle_at_expected(value, expected, context, built_elsewhere);
                    }
                }
            }
            ExpressionKind::Block(_, value) => {
                self.record_joined_type(expr, expected, context);
                self.settle_at_expected(value, expected, context, built_elsewhere);
            }
            ExpressionKind::Map(..) | ExpressionKind::EnumValue(..) => {
                self.record_joined_type(expr, expected, context);
            }
            // A built-in collection constructed from a literal (`List([..])`)
            // holds the literal's elements at its own element type.
            ExpressionKind::Call(callee, args) if self.names_a_collection(callee, expected) => {
                self.record_joined_type(expr, expected, context);
                for arg in args {
                    self.settle_at_expected(arg, expected, context, built_elsewhere);
                }
            }
            ExpressionKind::Call(callee, args) if self.is_variant_constructor(callee, context) => {
                self.record_joined_type(expr, expected, context);
                let payloads = self.variant_payloads_at(callee, expected, context);
                for (arg, payload) in args.iter().zip(payloads) {
                    self.settle_at_expected(arg, &payload, context, built_elsewhere);
                }
            }
            // The built-in optional constructor boxes its argument, which is
            // built at the optional's payload type.
            ExpressionKind::Call(callee, args) if self.builds_an_optional(expr, callee) => {
                self.record_joined_type(expr, expected, context);
                if let (TypeKind::Option(inner), [arg]) = (&expected.kind, args.as_slice()) {
                    self.settle_at_expected(arg, inner, context, built_elsewhere);
                }
            }
            // Every other expression reads a value already built at its own
            // type, which cannot be widened now: one left open where the
            // expected type binds an argument is reported.
            ExpressionKind::Identifier(..)
            | ExpressionKind::Member(..)
            | ExpressionKind::Index(..)
            | ExpressionKind::Call(..) => {
                let Some(actual) = self.get_type(expr.id).cloned() else {
                    return;
                };
                if !self
                    .inference_slots_bound_by(&actual, expected, context)
                    .is_empty()
                {
                    built_elsewhere.push((expr, actual));
                }
            }
            ExpressionKind::Type(..)
            | ExpressionKind::GenericType(..)
            | ExpressionKind::Literal(..)
            | ExpressionKind::Binary(..)
            | ExpressionKind::Logical(..)
            | ExpressionKind::Unary(..)
            | ExpressionKind::Assignment(..)
            | ExpressionKind::Range(..)
            | ExpressionKind::Guard(..)
            | ExpressionKind::ImportPath(..)
            | ExpressionKind::TypeDeclaration(..)
            | ExpressionKind::StructMember(..)
            | ExpressionKind::Lambda(..)
            | ExpressionKind::FormattedString(..)
            | ExpressionKind::NamedArgument(..)
            | ExpressionKind::Super
            | ExpressionKind::Cast(..) => {}
        }
    }

    /// Whether `call` is the built-in variant constructor that builds an
    /// optional, read from the callee fact the checker recorded — a user
    /// binding of the same name records no such fact.
    fn builds_an_optional(&self, call: &Expression, callee: &Expression) -> bool {
        matches!(
            self.fn_analysis.callee_kinds.get(&callee.id),
            Some(crate::type_checker::CalleeKind::VariantConstructor)
        ) && self
            .get_type(call.id)
            .is_some_and(|ty| matches!(ty.kind, TypeKind::Option(_)))
    }

    /// The payload types of the variant `callee` names, at the instantiation
    /// `expected` spells — what each argument of the constructor is stored at.
    /// Empty when `expected` is not an instantiation of the variant's enum.
    fn variant_payloads_at(
        &self,
        callee: &Expression,
        expected: &Type,
        context: &Context,
    ) -> Vec<Type> {
        let ExpressionKind::Member(owner, variant) = &callee.node else {
            return Vec::new();
        };
        let (ExpressionKind::Identifier(enum_name, _), ExpressionKind::Identifier(variant, _)) =
            (&owner.node, &variant.node)
        else {
            return Vec::new();
        };
        let Some(crate::type_checker::context::TypeDefinition::Enum(def)) = context
            .resolve_type_definition(enum_name)
            .or_else(|| self.type_definitions().get(enum_name))
        else {
            return Vec::new();
        };
        let (Some(generics), TypeKind::Custom(expected_name, Some(args))) =
            (def.generics.as_ref(), &expected.kind)
        else {
            return Vec::new();
        };
        if expected_name != enum_name || generics.len() != args.len() {
            return Vec::new();
        }
        let mapping: std::collections::HashMap<String, Type> = generics
            .iter()
            .zip(args)
            .filter_map(|(generic, arg)| {
                Some((
                    generic.name.clone(),
                    self.extract_type_from_expression(arg).ok()?,
                ))
            })
            .collect();
        def.variants
            .get(variant)
            .into_iter()
            .flatten()
            .map(|payload| self.substitute_type(payload, &mapping))
            .collect()
    }

    /// Whether `callee` names a variant of an enum (`E.L`), whose call builds
    /// the enum value at the type recorded for the call.
    fn is_variant_constructor(&self, callee: &Expression, context: &Context) -> bool {
        let ExpressionKind::Member(owner, _) = &callee.node else {
            return false;
        };
        let ExpressionKind::Identifier(name, _) = &owner.node else {
            return false;
        };
        matches!(
            context
                .resolve_type_definition(name)
                .or_else(|| self.type_definitions().get(name)),
            Some(crate::type_checker::context::TypeDefinition::Enum(_))
        )
    }

    /// Whether `callee` constructs the built-in collection `collection` is an
    /// instance of — the constructor that takes its elements as a literal.
    fn names_a_collection(&self, callee: &Expression, collection: &Type) -> bool {
        let (ExpressionKind::Identifier(called, _), TypeKind::Custom(name, _)) =
            (&callee.node, &collection.kind)
        else {
            return false;
        };
        called == name && BuiltinCollectionKind::from_name(name).is_some()
    }

    /// The element type a list, array or set literal of type `collection`
    /// holds, or `None` when `collection` is not one.
    fn literal_element_type(&self, collection: &Type) -> Option<Type> {
        let TypeKind::Custom(name, Some(args)) = &collection.kind else {
            return None;
        };
        BuiltinCollectionKind::from_name(name)?;
        self.extract_type_from_expression(args.first()?).ok()
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
    pub(crate) fn record_joined_type(
        &mut self,
        expr: &Expression,
        joined: &Type,
        context: &Context,
    ) {
        let Some(recorded) = self.get_type(expr.id) else {
            return;
        };
        let is_slot = |name: &str| {
            !matches!(
                context.resolve_type_definition(name),
                Some(crate::type_checker::context::TypeDefinition::Generic(_))
            )
        };
        let refined = fill_open_arguments_where(recorded, joined, &is_slot);
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
            self.record_joined_type(element, &element_type, context);
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
    fill_open_arguments_where(known, other, &|_| true)
}

/// [`fill_open_arguments`] filling only the open parameters `is_open` names.
///
/// A parameter the enclosing body declares stands for the body's own type
/// argument, which the general compatibility rule matches with anything; it
/// must never be filled from a sibling, or an identifier typed `T` in a
/// shared body would be recorded as whatever its first caller passes.
pub(crate) fn fill_open_arguments_where(
    known: &Type,
    other: &Type,
    is_open: &dyn Fn(&str) -> bool,
) -> Type {
    if let TypeKind::Generic(name, _, _) = &known.kind {
        return if matches!(other.kind, TypeKind::Generic(..)) || !is_open(name) {
            known.clone()
        } else {
            other.clone()
        };
    }
    let fill = |known: &Type, other: &Type| fill_open_arguments_where(known, other, is_open);
    let fill_all = |known: &[Expression], other: &[Expression]| -> Vec<Expression> {
        known
            .iter()
            .zip(other)
            .map(|(known, other)| fill_open_expression(known, other, is_open))
            .collect()
    };
    let fill_one =
        |known: &Expression, other: &Expression| fill_open_expression(known, other, is_open);
    let kind = match (&known.kind, &other.kind) {
        (TypeKind::Custom(name, Some(args)), TypeKind::Custom(other_name, Some(other_args)))
            if name == other_name && args.len() == other_args.len() =>
        {
            TypeKind::Custom(name.clone(), Some(fill_all(args, other_args)))
        }
        (TypeKind::Option(inner), TypeKind::Option(other_inner)) => {
            TypeKind::Option(Box::new(fill(inner, other_inner)))
        }
        (TypeKind::Tuple(parts), TypeKind::Tuple(other_parts))
            if parts.len() == other_parts.len() =>
        {
            TypeKind::Tuple(fill_all(parts, other_parts))
        }
        (TypeKind::List(element), TypeKind::List(other_element)) => {
            TypeKind::List(Box::new(fill_one(element, other_element)))
        }
        (TypeKind::Set(element), TypeKind::Set(other_element)) => {
            TypeKind::Set(Box::new(fill_one(element, other_element)))
        }
        (TypeKind::Map(key, value), TypeKind::Map(other_key, other_value)) => TypeKind::Map(
            Box::new(fill_one(key, other_key)),
            Box::new(fill_one(value, other_value)),
        ),
        (TypeKind::Result(ok, err), TypeKind::Result(other_ok, other_err)) => TypeKind::Result(
            Box::new(fill_one(ok, other_ok)),
            Box::new(fill_one(err, other_err)),
        ),
        // Every other pairing either names no argument to fill or is two
        // different types, which the element compatibility check reports.
        (known_kind, _) => known_kind.clone(),
    };
    Type::new(kind, known.span)
}

/// A type argument written as an expression, filled like
/// [`fill_open_arguments_where`]; a value argument (an array's size) is kept.
fn fill_open_expression(
    known: &Expression,
    other: &Expression,
    is_open: &dyn Fn(&str) -> bool,
) -> Expression {
    let (ExpressionKind::Type(known_ty, nullable), ExpressionKind::Type(other_ty, _)) =
        (&known.node, &other.node)
    else {
        return known.clone();
    };
    let mut filled = known.clone();
    filled.node = ExpressionKind::Type(
        Box::new(fill_open_arguments_where(known_ty, other_ty, is_open)),
        *nullable,
    );
    filled
}
