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
use crate::ast::types::{BuiltinCollectionKind, FunctionTypeData, Type, TypeKind};
use crate::ast::{BinaryOp, Expression, ExpressionKind, UnaryOp};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use std::collections::{HashMap, HashSet};

/// Sentinel `TypeKind::Custom` name used to smuggle a value-generic argument
/// (e.g. the `3` in `Foo<float, 3>`) through the existing
/// `HashMap<String, Type>` substitution map. Generic params can be either
/// type-typed or value-typed in their declared class, but `substitute_type`
/// is keyed by name → `Type`; wrapping the value expression as
/// `Custom("__value_generic__", Some([expr]))` lets us look up value
/// generics out of the same map in size-expression positions
/// (`substitute_value_generic_in_expr`) without threading a second mapping
/// through every callsite of `substitute_type`.
pub(crate) use crate::ast::types::VALUE_GENERIC_MARKER;

/// Upper bound on `substitute_type` recursion depth. Bounds stack usage when a
/// deeply nested generic type (hostile or generated) flows through
/// substitution; past this the descent stops and returns the input unchanged.
const MAX_SUBSTITUTION_DEPTH: usize = 256;

thread_local! {
    static SUBSTITUTION_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Resolve a declared field type through an instantiation's type arguments.
///
/// A field typed as a bare generic parameter (`TypeKind::Generic("T", …)` or
/// `TypeKind::Custom("T", None)`) is replaced by the concrete type argument at
/// the parameter's declaration position, so a `Box<float>` field lays out at
/// the concrete scalar width instead of a pointer slot. Fields with a concrete
/// type, or an unresolved generic (no matching argument), are returned
/// unchanged — callers that must distinguish the two use
/// [`is_generic_parameter_kind`] on the result.
///
/// Used for class and struct fields during layout, and for enum variant
/// payloads on both sides of the store/load pair.
pub(crate) fn substitute_generic_field_kind(
    field_kind: &TypeKind,
    type_args: Option<&[Expression]>,
    def_generics: Option<&Vec<GenericDefinition>>,
) -> TypeKind {
    // Only a bare generic-parameter spelling can be substituted; a concrete
    // field type is returned unchanged.
    let Some(param_name) = generic_parameter_name(field_kind) else {
        return field_kind.clone();
    };
    let (Some(generics), Some(args)) = (def_generics, type_args) else {
        return field_kind.clone();
    };
    let Some(pos) = generics.iter().position(|g| g.name == param_name) else {
        return field_kind.clone();
    };
    if let Some(ExpressionKind::Type(ty, _)) = args.get(pos).map(|a| &a.node) {
        ty.kind.clone()
    } else {
        field_kind.clone()
    }
}

/// Returns true when `kind` still names a generic parameter of `def_generics`,
/// i.e. [`substitute_generic_field_kind`] could not resolve it to a concrete
/// type. Such a type has no known width, so callers must fall back to the
/// pointer-sized representation rather than coercing to it.
pub(crate) fn is_generic_parameter_kind(
    kind: &TypeKind,
    def_generics: Option<&Vec<GenericDefinition>>,
) -> bool {
    let Some(param_name) = generic_parameter_name(kind) else {
        return false;
    };
    def_generics
        .map(|generics| generics.iter().any(|g| g.name == param_name))
        .unwrap_or(false)
}

/// The parameter name a bare generic-parameter type spelling refers to, or
/// `None` for any concrete type.
pub(crate) fn generic_parameter_name(kind: &TypeKind) -> Option<&str> {
    if let TypeKind::Generic(name, _, _) = kind {
        Some(name.as_str())
    } else if let TypeKind::Custom(name, None) = kind {
        Some(name.as_str())
    } else {
        None
    }
}

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
    extract_value_generic_kind(&ty.kind)
}

/// [`extract_value_generic`] against a bare kind, for callers that hold one
/// without the surrounding [`Type`] — the symbol mangler reads kinds.
pub(crate) fn extract_value_generic_kind(kind: &TypeKind) -> Option<&Expression> {
    match kind {
        TypeKind::Custom(name, Some(args)) if name == VALUE_GENERIC_MARKER && args.len() == 1 => {
            Some(&args[0])
        }
        _ => None,
    }
}

/// The instantiation-registry slot a generic argument fills when it is a value
/// rather than a type.
///
/// A class can declare either kind of parameter, and an instantiation is
/// recorded as one `Vec<Type>` regardless — so the `3` in `Array<T, 3>` rides
/// in the same tuple as a value-generic marker. Only a folded integer constant
/// qualifies: a size still written as an expression names no single
/// instantiation, and wrapping it would mangle two different sizes to one
/// symbol.
pub(crate) fn value_generic_slot(arg: &Expression) -> Option<Type> {
    match &arg.node {
        ExpressionKind::Literal(crate::ast::literal::Literal::Integer(_)) => {
            Some(value_generic_marker_type(arg.clone()))
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

/// A value argument computed from value parameters — the `Size + 1` in
/// `Buf<T, Size + 1>` — folded to the integer literal it denotes once
/// `mapping` binds each parameter it names to a constant.
///
/// `None` when the expression names something `mapping` does not bind to a
/// value, or does not evaluate to an integer: it then names no single
/// instantiation and stays as written.
pub(crate) fn fold_value_generic_arithmetic(
    expr: &Expression,
    mapping: &HashMap<String, Type>,
) -> Option<Expression> {
    let substituted = substitute_value_names(expr, mapping)?;
    let value = TypeChecker::try_eval_const_int(&substituted)?;
    Some(crate::ast::factory::literal_with_span(
        crate::ast::factory::int_literal(value),
        expr.span,
    ))
}

/// Why a value argument whose every parameter is bound has no value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnfoldableValue {
    /// An operand that is neither an integer nor a value parameter of the
    /// body: nothing the instantiation binds gives it a value.
    NotConstant,
    /// A division or remainder by an operand that is zero.
    DivisionByZero,
    /// An operator the folder does not evaluate (`<<`, `==`, …).
    UnsupportedOperator,
    /// A result, or an operand, outside the signed 128-bit range.
    OutOfRange,
}

/// Why the value argument `expr` has no value once `mapping` binds every
/// parameter it names. `None` when it folds, or when it names a parameter of
/// `open` — one the body declares and `mapping` leaves unbound — since then it
/// is not yet a value at all. An operand that is neither an integer, a
/// parameter `mapping` binds to a value, nor one of `open` has no value
/// whatever is bound, and is [`UnfoldableValue::NotConstant`].
pub(crate) fn unfoldable_value_argument(
    expr: &Expression,
    mapping: &HashMap<String, Type>,
    open: &HashSet<String>,
) -> Option<UnfoldableValue> {
    match value_operands(expr, mapping, open) {
        ValueOperands::Foreign => return Some(UnfoldableValue::NotConstant),
        ValueOperands::Open => return None,
        ValueOperands::Bound => {}
    }
    let substituted = substitute_value_names(expr, mapping)?;
    if TypeChecker::try_eval_const_int(&substituted).is_some() {
        return None;
    }
    Some(if divides_by_zero(&substituted) {
        UnfoldableValue::DivisionByZero
    } else if has_unsupported_operator(&substituted) {
        UnfoldableValue::UnsupportedOperator
    } else {
        UnfoldableValue::OutOfRange
    })
}

/// What the operands of a value argument are, the worst of them deciding: an
/// argument with one foreign operand is foreign however the rest are bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ValueOperands {
    /// Every operand is an integer or a parameter bound to a value.
    Bound,
    /// Some operand is a parameter the body declares and leaves unbound.
    Open,
    /// Some operand is neither: it has no value at any instantiation.
    Foreign,
}

/// Classify the operands of the value argument `expr` against the parameters
/// `mapping` binds and the ones `open` leaves unbound.
fn value_operands(
    expr: &Expression,
    mapping: &HashMap<String, Type>,
    open: &HashSet<String>,
) -> ValueOperands {
    match &expr.node {
        ExpressionKind::Binary(left, _, right) => {
            value_operands(left, mapping, open).max(value_operands(right, mapping, open))
        }
        ExpressionKind::Unary(_, inner) => value_operands(inner, mapping, open),
        ExpressionKind::Literal(crate::ast::literal::Literal::Integer(_)) => ValueOperands::Bound,
        _ => match written_name(expr) {
            Some(name) if bound_value(name, mapping).is_some() => ValueOperands::Bound,
            Some(name) if open.contains(name) => ValueOperands::Open,
            Some(_) | None => ValueOperands::Foreign,
        },
    }
}

/// Whether `expr` divides, or takes a remainder, by an operand that folds to
/// zero.
fn divides_by_zero(expr: &Expression) -> bool {
    if let ExpressionKind::Binary(left, op, right) = &expr.node {
        let by_zero = matches!(op, BinaryOp::Div | BinaryOp::Mod)
            && TypeChecker::try_eval_const_int(right) == Some(0);
        return by_zero || divides_by_zero(left) || divides_by_zero(right);
    }
    if let ExpressionKind::Unary(_, inner) = &expr.node {
        return divides_by_zero(inner);
    }
    false
}

/// Whether `expr` applies an operator [`TypeChecker::try_eval_const_int`]
/// does not evaluate.
fn has_unsupported_operator(expr: &Expression) -> bool {
    if let ExpressionKind::Binary(left, op, right) = &expr.node {
        return !is_folded_binary_operator(*op)
            || has_unsupported_operator(left)
            || has_unsupported_operator(right);
    }
    if let ExpressionKind::Unary(op, inner) = &expr.node {
        return !is_folded_unary_operator(*op) || has_unsupported_operator(inner);
    }
    false
}

/// Whether the constant folder evaluates the binary operator `op`.
fn is_folded_binary_operator(op: BinaryOp) -> bool {
    match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => true,
        BinaryOp::BitwiseOr
        | BinaryOp::BitwiseAnd
        | BinaryOp::BitwiseXor
        | BinaryOp::Equal
        | BinaryOp::NotEqual
        | BinaryOp::LessThan
        | BinaryOp::LessThanEqual
        | BinaryOp::GreaterThan
        | BinaryOp::GreaterThanEqual
        | BinaryOp::Not
        | BinaryOp::And
        | BinaryOp::Or
        | BinaryOp::Range
        | BinaryOp::In
        | BinaryOp::NullCoalesce => false,
    }
}

/// Whether the constant folder evaluates the unary operator `op`.
fn is_folded_unary_operator(op: UnaryOp) -> bool {
    match op {
        UnaryOp::Negate | UnaryOp::Plus => true,
        UnaryOp::Not
        | UnaryOp::BitwiseNot
        | UnaryOp::Decrement
        | UnaryOp::Increment
        | UnaryOp::Await => false,
    }
}

/// Each value parameter `expr` names that `mapping` binds, with the value it
/// is bound to, in the order `expr` first names them.
pub(crate) fn bound_value_names<'e>(
    expr: &'e Expression,
    mapping: &'e HashMap<String, Type>,
) -> Vec<(&'e str, &'e Expression)> {
    let mut found: Vec<(&str, &Expression)> = Vec::new();
    collect_bound_value_names(expr, mapping, &mut found);
    found
}

fn collect_bound_value_names<'e>(
    expr: &'e Expression,
    mapping: &'e HashMap<String, Type>,
    found: &mut Vec<(&'e str, &'e Expression)>,
) {
    if let Some(name) = written_name(expr) {
        let bound = mapping.get(name).and_then(extract_value_generic);
        if let Some(value) = bound.filter(|_| found.iter().all(|(seen, _)| *seen != name)) {
            found.push((name, value));
        }
        return;
    }
    if let ExpressionKind::Binary(left, _, right) = &expr.node {
        collect_bound_value_names(left, mapping, found);
        collect_bound_value_names(right, mapping, found);
    } else if let ExpressionKind::Unary(_, inner) = &expr.node {
        collect_bound_value_names(inner, mapping, found);
    }
}

/// `expr` with each value parameter it names replaced by the value `mapping`
/// binds it to; `None` when it names anything else, or is not arithmetic.
fn substitute_value_names(
    expr: &Expression,
    mapping: &HashMap<String, Type>,
) -> Option<Expression> {
    if let Some(name) = written_name(expr) {
        return bound_value(name, mapping);
    }
    let rebuilt = |node: ExpressionKind| Expression {
        id: expr.id,
        span: expr.span,
        node,
    };
    if let ExpressionKind::Binary(left, op, right) = &expr.node {
        return Some(rebuilt(ExpressionKind::Binary(
            Box::new(substitute_value_names(left, mapping)?),
            *op,
            Box::new(substitute_value_names(right, mapping)?),
        )));
    }
    if let ExpressionKind::Unary(op, inner) = &expr.node {
        return Some(rebuilt(ExpressionKind::Unary(
            *op,
            Box::new(substitute_value_names(inner, mapping)?),
        )));
    }
    matches!(
        &expr.node,
        ExpressionKind::Literal(crate::ast::literal::Literal::Integer(_))
    )
    .then(|| expr.clone())
}

/// A value argument written as arithmetic over value parameters and named
/// constants — the `Size + K` in `Buf<T, Size + K>` — with every constant
/// replaced by its value, so the argument folds to one instantiation once the
/// parameters are bound. A value parameter in scope shadows a constant of its
/// name and stays as written, as does anything that is not arithmetic.
pub(crate) fn inline_value_constants(expr: &Expression, context: &Context) -> Expression {
    let rebuilt = |node: ExpressionKind| Expression {
        id: expr.id,
        span: expr.span,
        node,
    };
    if let ExpressionKind::Binary(left, op, right) = &expr.node {
        return rebuilt(ExpressionKind::Binary(
            Box::new(inline_value_constants(left, context)),
            *op,
            Box::new(inline_value_constants(right, context)),
        ));
    }
    if let ExpressionKind::Unary(op, inner) = &expr.node {
        return rebuilt(ExpressionKind::Unary(
            *op,
            Box::new(inline_value_constants(inner, context)),
        ));
    }
    match written_name(expr).and_then(|name| constant_value(name, context)) {
        Some(value) => crate::ast::factory::literal_with_span(
            crate::ast::factory::int_literal(value),
            expr.span,
        ),
        None => expr.clone(),
    }
}

/// The first operand of the value argument `expr` that is not a compile-time
/// constant, or `None` when every operand is one.
///
/// An operand is a compile-time constant when it is an integer literal, a name
/// [`TypeChecker::resolve_const_int`] folds, or a generic parameter in scope —
/// which the instantiation it runs at binds to a value. The operators are left
/// to the fold that binds those parameters. Anything else — a binding computed
/// while the program runs, a call, a field read — has no value until then, so
/// an argument built from it names no single instantiation.
pub(crate) fn non_constant_value_operand<'e>(
    expr: &'e Expression,
    context: &Context,
) -> Option<&'e Expression> {
    match &expr.node {
        ExpressionKind::Binary(left, _, right) => non_constant_value_operand(left, context)
            .or_else(|| non_constant_value_operand(right, context)),
        ExpressionKind::Unary(_, inner) => non_constant_value_operand(inner, context),
        ExpressionKind::Literal(crate::ast::literal::Literal::Integer(_)) => None,
        _ => match written_name(expr) {
            Some(name) if is_constant_name(name, context) => None,
            Some(_) | None => Some(expr),
        },
    }
}

/// Whether `name` is a generic parameter in scope or a name that folds to an
/// integer constant.
fn is_constant_name(name: &str, context: &Context) -> bool {
    matches!(
        context.resolve_type_definition(name),
        Some(TypeDefinition::Generic(_))
    ) || TypeChecker::resolve_const_int(name, Some(context)).is_some()
}

impl TypeChecker {
    /// Report `operand`, part of the value argument of `class`, as not being a
    /// compile-time constant.
    pub(crate) fn report_non_constant_value_argument(&mut self, class: &str, operand: &Expression) {
        self.report_error_with_help(
            DiagnosticCode::TypNonConstantValueArgument,
            format!(
                "`{}` is not a compile-time constant, so the value argument of `{class}` names no instantiation",
                crate::ast::formatter::expression_text(operand)
            ),
            operand.span,
            NON_CONSTANT_VALUE_HELP.to_string(),
        );
    }
}

/// The help a non-constant value argument is reported with.
const NON_CONSTANT_VALUE_HELP: &str = "build a value argument from integer literals, `const`s and the value parameters in scope; keep a value known only while the program runs in a field instead of in the type";

/// The integer constant `name` binds in `context`, unless a generic parameter
/// in scope shadows it.
fn constant_value(name: &str, context: &Context) -> Option<i128> {
    if matches!(
        context.resolve_type_definition(name),
        Some(TypeDefinition::Generic(_))
    ) {
        return None;
    }
    TypeChecker::resolve_const_int(name, Some(context))
}

/// The bare name `expr` writes, as an identifier or as a type argument
/// naming no type (`Size` in `Array<T, Size>`).
fn written_name(expr: &Expression) -> Option<&str> {
    if let ExpressionKind::Identifier(name, None) = &expr.node {
        return Some(name);
    }
    let ExpressionKind::Type(ty, false) = &expr.node else {
        return None;
    };
    let TypeKind::Custom(name, None) = &ty.kind else {
        return None;
    };
    Some(name)
}

/// The value `mapping` binds the value parameter `name` to.
fn bound_value(name: &str, mapping: &HashMap<String, Type>) -> Option<Expression> {
    mapping.get(name).and_then(extract_value_generic).cloned()
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
            BuiltinCollectionKind::List.name().to_string(),
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
            BuiltinCollectionKind::Map.name().to_string(),
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
            BuiltinCollectionKind::Set.name().to_string(),
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
            BuiltinCollectionKind::Array.name().to_string(),
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
        // Every recursive descent (through the helpers below) re-enters here, so
        // one guard at this choke point bounds total nesting. A hostile source
        // type nested past the cap would otherwise exhaust the stack; on exceed
        // we stop substituting and return the input verbatim (fail-safe — any
        // resulting type mismatch surfaces through normal checking, never a
        // panic).
        let depth = SUBSTITUTION_DEPTH.with(|d| {
            let current = d.get();
            d.set(current + 1);
            current
        });
        if depth >= MAX_SUBSTITUTION_DEPTH {
            SUBSTITUTION_DEPTH.with(|d| d.set(d.get() - 1));
            return ty.clone();
        }
        let result = match &ty.kind {
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
        };
        SUBSTITUTION_DEPTH.with(|d| d.set(d.get() - 1));
        result
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
            BuiltinCollectionKind::List.name().to_string(),
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
            BuiltinCollectionKind::Map.name().to_string(),
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
            BuiltinCollectionKind::Set.name().to_string(),
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
            BuiltinCollectionKind::Array.name().to_string(),
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
            self.report_error(
                DiagnosticCode::TypGenericArgumentCount,
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
                            DiagnosticCode::TypGenericTypeStructure,
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

// Stays inline rather than moving to `tests/type_checker/generics.rs`: both
// `substitute_type` and `MAX_SUBSTITUTION_DEPTH` are crate-internal, and the
// nesting depth this exercises is not expressible in a `.mi` source program.
#[cfg(test)]
mod substitution_depth_tests {
    use super::*;
    use crate::type_checker::TypeChecker;

    #[test]
    fn substitute_type_bounds_recursion_on_deep_nesting() {
        // Nest far past MAX_SUBSTITUTION_DEPTH so an unbounded implementation
        // would blow the stack; the depth guard must return instead.
        let mut ty = make_type(TypeKind::Int);
        for _ in 0..(MAX_SUBSTITUTION_DEPTH * 8) {
            ty = make_type(TypeKind::Option(Box::new(ty)));
        }

        let checker = TypeChecker::new();
        let mapping = HashMap::new();
        let result = checker.substitute_type(&ty, &mapping);

        // Reaching here without a stack overflow is the assertion; the result
        // is still an Option envelope (substitution stopped, did not corrupt).
        assert!(matches!(result.kind, TypeKind::Option(_)));
    }
}

#[cfg(test)]
mod value_argument_tests {
    use super::*;
    use crate::ast::literal::{IntegerLiteral, Literal};
    use crate::ast::IdNode;

    fn node(kind: ExpressionKind) -> Expression {
        IdNode::new(0, kind, Span::new(0, 0))
    }

    fn integer(value: i64) -> Expression {
        node(ExpressionKind::Literal(Literal::Integer(
            IntegerLiteral::I64(value),
        )))
    }

    fn name(name: &str) -> Expression {
        node(ExpressionKind::Identifier(name.to_string(), None))
    }

    fn plus(left: Expression, right: Expression) -> Expression {
        node(ExpressionKind::Binary(
            Box::new(left),
            BinaryOp::Add,
            Box::new(right),
        ))
    }

    fn size_bound_to(value: i64) -> HashMap<String, Type> {
        HashMap::from([(
            "Size".to_string(),
            value_generic_marker_type(integer(value)),
        )])
    }

    /// An operand bound to a value folds; one the body declares but leaves
    /// unbound is not yet a value; one the body does not declare is never one.
    #[test]
    fn only_a_declared_parameter_keeps_a_value_argument_open() {
        let bound = size_bound_to(2);
        let open = HashSet::from(["Rest".to_string()]);
        assert_eq!(
            unfoldable_value_argument(&plus(name("Size"), integer(1)), &bound, &open),
            None
        );
        assert_eq!(
            unfoldable_value_argument(&plus(name("Size"), name("Rest")), &bound, &open),
            None
        );
        assert_eq!(
            unfoldable_value_argument(&plus(name("Size"), name("m")), &bound, &open),
            Some(UnfoldableValue::NotConstant)
        );
    }

    /// A foreign operand decides the answer even beside an open parameter.
    #[test]
    fn a_foreign_operand_beside_an_open_one_is_not_constant() {
        let open = HashSet::from(["Rest".to_string()]);
        assert_eq!(
            unfoldable_value_argument(&plus(name("Rest"), name("m")), &HashMap::new(), &open),
            Some(UnfoldableValue::NotConstant)
        );
    }
}
