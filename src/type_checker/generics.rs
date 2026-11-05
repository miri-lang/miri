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
use crate::ast::type_arguments::{rewritten, ArithOp, TypeArgKind, TypeArgument, ValueArg};
use crate::ast::types::{BuiltinCollectionKind, FunctionTypeData, Type, TypeKind};
use crate::ast::{Expression, ExpressionKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use std::collections::{HashMap, HashSet};

/// Sentinel `TypeKind::Custom` name used to smuggle a value-generic argument
/// (e.g. the `3` in `Foo<float, 3>`) through the existing
/// `HashMap<String, Type>` substitution map. Generic params can be either
/// type-typed or value-typed in their declared class, but `substitute_type`
/// is keyed by name → `Type`; wrapping the value expression as
/// `Custom("__value_generic__", Some([argument]))` lets us look up value
/// generics out of the same map in size positions
/// (`substitute_value_generic`) without threading a second mapping
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
/// the parameter's declaration position. Fields with a concrete type, or an
/// unresolved generic (no matching argument), are returned unchanged — callers
/// that must distinguish the two use [`is_generic_parameter_kind`] on the
/// result.
///
/// Only a bare parameter is substituted, and a nullable argument is not
/// folded into the optional it denotes. That is enough for the type checker's
/// own questions about a payload — whether it can be compared structurally or
/// interpolated — but not for storage: every layout, binding and release
/// reads a member at an instantiation through
/// `mir::instantiation::member_type_at`, which substitutes the whole type.
pub(crate) fn substitute_generic_field_kind(
    field_kind: &TypeKind,
    type_args: Option<&[TypeArgument]>,
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
    if let Some(TypeArgKind::Type(ty, _)) = args.get(pos).map(|a| &a.node) {
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

/// Wrap a value-generic argument as a sentinel `Type` so it can share the
/// `HashMap<String, Type>` mapping used for type generics.
pub(crate) fn value_generic_marker_type(argument: TypeArgument) -> Type {
    make_type(TypeKind::Custom(
        VALUE_GENERIC_MARKER.to_string(),
        Some(vec![argument]),
    ))
}

/// If `ty` is a value-generic marker wrapping a stored argument, return a
/// borrow of that argument. Otherwise `None`.
pub(crate) fn extract_value_generic(ty: &Type) -> Option<&TypeArgument> {
    extract_value_generic_kind(&ty.kind)
}

/// [`extract_value_generic`] against a bare kind, for callers that hold one
/// without the surrounding [`Type`] — the symbol mangler reads kinds.
pub(crate) fn extract_value_generic_kind(kind: &TypeKind) -> Option<&TypeArgument> {
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
/// in the same tuple as a value-generic marker. Only a folded integer
/// qualifies: a size still written as arithmetic or a name names no single
/// instantiation, and wrapping it would mangle two different sizes to one
/// symbol.
pub(crate) fn value_generic_slot(arg: &TypeArgument) -> Option<Type> {
    match &arg.node {
        TypeArgKind::Value(ValueArg::Int(_)) => Some(value_generic_marker_type(arg.clone())),
        TypeArgKind::Value(
            ValueArg::Name(_) | ValueArg::Binary(..) | ValueArg::OutOfRange { .. },
        )
        | TypeArgKind::Type(..) => None,
    }
}

/// `arg` with a value generic it names replaced by the argument `mapping`
/// binds it to. A name `mapping` binds to a type flows through
/// `substitute_type` instead, and anything else is kept as written. Used by
/// `substitute_array` (and any other size-position substitution) so that
/// `Array<T, Size>` inside a class body lowers to `Array<float, 3>` when the
/// class is instantiated as `Wrap<float, 3>`.
pub(crate) fn substitute_value_generic(
    arg: &TypeArgument,
    mapping: &HashMap<String, Type>,
) -> TypeArgument {
    let bound = written_name(arg)
        .and_then(|name| mapping.get(name))
        .and_then(extract_value_generic);
    bound.unwrap_or(arg).clone()
}

/// A value argument computed from value parameters — the `Size + 1` in
/// `Buf<T, Size + 1>` — folded to the integer it denotes once `mapping` binds
/// each parameter it names to a value.
///
/// `None` when the argument names something `mapping` does not bind to a
/// value, or does not evaluate to an integer: it then names no single
/// instantiation and stays as written.
pub(crate) fn fold_value_generic_arithmetic(
    arg: &TypeArgument,
    mapping: &HashMap<String, Type>,
) -> Option<TypeArgument> {
    let value = written_value(arg)?.fold(&mut |name| bound_int(name, mapping))?;
    Some(rewritten(arg, TypeArgKind::Value(ValueArg::Int(value))))
}

/// Why a value argument whose every parameter is bound has no valid value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnfoldableValue {
    /// An operand that is neither an integer nor a value parameter of the
    /// body: nothing the instantiation binds gives it a value.
    NotConstant,
    /// A division or remainder by an operand that is zero.
    DivisionByZero,
    /// A result, or an operand, outside the signed 128-bit range.
    OutOfRange,
    /// A result of zero or below: a value argument counts what an instance
    /// holds.
    NotPositive,
}

/// Why the value argument `arg` has no valid value once `mapping` binds every
/// parameter it names. `None` when it folds to a value above zero, or when it
/// names a parameter of `open` — one the body declares and `mapping` leaves
/// unbound — since then it is not yet a value at all. An operand that is
/// neither an integer, a parameter `mapping` binds to a value, nor one of
/// `open` has no value whatever is bound, and is
/// [`UnfoldableValue::NotConstant`].
pub(crate) fn unfoldable_value_argument(
    arg: &TypeArgument,
    mapping: &HashMap<String, Type>,
    open: &HashSet<String>,
) -> Option<UnfoldableValue> {
    let Some(value) = written_value(arg) else {
        return Some(UnfoldableValue::NotConstant);
    };
    match value_operands(&value, mapping, open) {
        ValueOperands::Foreign => return Some(UnfoldableValue::NotConstant),
        ValueOperands::Open => return None,
        ValueOperands::Bound => {}
    }
    match value.fold(&mut |name| bound_int(name, mapping)) {
        Some(folded) if folded > 0 => return None,
        Some(_) => return Some(UnfoldableValue::NotPositive),
        None => {}
    }
    Some(if divides_by_zero(&value, mapping) {
        UnfoldableValue::DivisionByZero
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

/// Classify the operands of `value` against the parameters `mapping` binds and
/// the ones `open` leaves unbound.
fn value_operands(
    value: &ValueArg,
    mapping: &HashMap<String, Type>,
    open: &HashSet<String>,
) -> ValueOperands {
    match value {
        ValueArg::Int(_) | ValueArg::OutOfRange { .. } => ValueOperands::Bound,
        ValueArg::Name(name) if bound_argument(name, mapping).is_some() => ValueOperands::Bound,
        ValueArg::Name(name) if open.contains(name) => ValueOperands::Open,
        ValueArg::Name(_) => ValueOperands::Foreign,
        ValueArg::Binary(_, left, right) => {
            value_operands(left, mapping, open).max(value_operands(right, mapping, open))
        }
    }
}

/// Whether `value` divides, or takes a remainder, by an operand that folds to
/// zero once `mapping` binds the parameters it names.
fn divides_by_zero(value: &ValueArg, mapping: &HashMap<String, Type>) -> bool {
    let ValueArg::Binary(op, left, right) = value else {
        return false;
    };
    let by_zero = matches!(op, ArithOp::Div | ArithOp::Mod)
        && right.fold(&mut |name| bound_int(name, mapping)) == Some(0);
    by_zero || divides_by_zero(left, mapping) || divides_by_zero(right, mapping)
}

/// Each value parameter `arg` names that `mapping` binds, with the argument
/// it is bound to, in the order `arg` first names them.
pub(crate) fn bound_value_names<'e>(
    arg: &'e TypeArgument,
    mapping: &'e HashMap<String, Type>,
) -> Vec<(&'e str, &'e TypeArgument)> {
    let mut names = Vec::new();
    match &arg.node {
        TypeArgKind::Value(value) => collect_value_names(value, &mut names),
        TypeArgKind::Type(..) => names.extend(written_name(arg)),
    }
    let mut found: Vec<(&str, &TypeArgument)> = Vec::new();
    for name in names {
        let bound = bound_argument(name, mapping);
        if let Some(value) = bound.filter(|_| found.iter().all(|(seen, _)| *seen != name)) {
            found.push((name, value));
        }
    }
    found
}

/// The names `value` is written with, in order.
fn collect_value_names<'e>(value: &'e ValueArg, names: &mut Vec<&'e str>) {
    match value {
        ValueArg::Int(_) | ValueArg::OutOfRange { .. } => {}
        ValueArg::Name(name) => names.push(name),
        ValueArg::Binary(_, left, right) => {
            collect_value_names(left, names);
            collect_value_names(right, names);
        }
    }
}

/// A value argument written with named constants — the `Size + K` in
/// `Buf<T, Size + K>` — with every constant replaced by its value, so the
/// argument folds to one instantiation once the parameters are bound. A value
/// parameter in scope shadows a constant of its name and stays as written, as
/// does a type.
pub(crate) fn inline_value_constants(arg: &TypeArgument, context: &Context) -> TypeArgument {
    let Some(value) = written_value(arg) else {
        return arg.clone();
    };
    let inlined = value.substituted(&|name| constant_value(name, context).map(ValueArg::Int));
    if matches!(inlined, ValueArg::Name(_)) {
        // A bare name no constant binds stays the type-or-value name it was
        // written as.
        return arg.clone();
    }
    rewritten(arg, TypeArgKind::Value(inlined))
}

/// The first operand of the value argument `arg` that is not a compile-time
/// constant, or `None` when every operand is one.
///
/// An operand is a compile-time constant when it is an integer, a name
/// [`TypeChecker::resolve_const_int`] folds, or a generic parameter in scope —
/// which the instantiation it runs at binds to a value. The operators are left
/// to the fold that binds those parameters. A binding computed while the
/// program runs has no value until then, so an argument built from it names no
/// single instantiation.
pub(crate) fn non_constant_value_operand(arg: &TypeArgument, context: &Context) -> Option<String> {
    let mut names = Vec::new();
    match &arg.node {
        TypeArgKind::Value(value) => collect_value_names(value, &mut names),
        TypeArgKind::Type(..) => names.extend(written_name(arg)),
    }
    names
        .into_iter()
        .find(|name| !is_constant_name(name, context))
        .map(str::to_string)
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
    /// Report `operand`, named in the value argument of `class` written at
    /// `span`, as not being a compile-time constant.
    pub(crate) fn report_non_constant_value_argument(
        &mut self,
        class: &str,
        operand: &str,
        span: Span,
    ) {
        self.report_error_with_help(
            DiagnosticCode::TypNonConstantValueArgument,
            format!(
                "`{operand}` is not a compile-time constant, so the value argument of `{class}` names no instantiation"
            ),
            span,
            NON_CONSTANT_VALUE_HELP.to_string(),
        );
    }

    /// Report the integer `written`, a value argument of `class` written at
    /// `span`, as not fitting in the 128 bits a value argument holds.
    pub(crate) fn report_value_argument_out_of_range(
        &mut self,
        class: &str,
        written: &str,
        span: Span,
    ) {
        self.report_error(
            DiagnosticCode::TypIntegerLiteralOutOfRange,
            format!(
                "`{written}` does not fit in a 128-bit integer, so the value argument of `{class}` names no instantiation"
            ),
            span,
        );
    }

    /// Report `arg`, a value argument of `class` that folds to `value`, as not
    /// being greater than zero; the span under `arg` shows which argument.
    pub(crate) fn report_non_positive_value_argument(
        &mut self,
        class: &str,
        arg: &TypeArgument,
        value: i128,
    ) {
        self.report_error_with_help(
            DiagnosticCode::TypNonPositiveValueArgument,
            format!(
                "the value argument of `{class}` is {value}; a value argument must be greater than zero"
            ),
            arg.span,
            NON_POSITIVE_VALUE_HELP.to_string(),
        );
    }
}

/// The help a non-positive value argument is reported with.
const NON_POSITIVE_VALUE_HELP: &str = "a value argument counts what each instance holds, so it is at least 1; pass a count that may be zero or negative to the constructor instead";

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

/// The bare name `arg` writes: a value naming one, or a type argument naming
/// no type (`Size` in `Array<T, Size>` parses as one).
fn written_name(arg: &TypeArgument) -> Option<&str> {
    match &arg.node {
        TypeArgKind::Value(ValueArg::Name(name)) => Some(name),
        TypeArgKind::Type(ty, false) => match &ty.kind {
            TypeKind::Custom(name, None) => Some(name),
            _ => None,
        },
        TypeArgKind::Value(
            ValueArg::Int(_) | ValueArg::Binary(..) | ValueArg::OutOfRange { .. },
        )
        | TypeArgKind::Type(_, true) => None,
    }
}

/// The value `arg` writes, a bare name included; `None` for a type.
fn written_value(arg: &TypeArgument) -> Option<ValueArg> {
    match &arg.node {
        TypeArgKind::Value(value) => Some(value.clone()),
        TypeArgKind::Type(..) => written_name(arg).map(|name| ValueArg::Name(name.to_string())),
    }
}

/// The argument `mapping` binds the value parameter `name` to.
fn bound_argument<'m>(name: &str, mapping: &'m HashMap<String, Type>) -> Option<&'m TypeArgument> {
    mapping.get(name).and_then(extract_value_generic)
}

/// The integer `mapping` binds the value parameter `name` to: the argument it
/// is bound to, folded with no names of its own.
fn bound_int(name: &str, mapping: &HashMap<String, Type>) -> Option<i128> {
    match &bound_argument(name, mapping)?.node {
        TypeArgKind::Value(value) => value.fold(&mut |_| None),
        TypeArgKind::Type(..) => None,
    }
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

            // A trait or base class matches a type below it at the arguments
            // the clauses on the way up pass it.
            (TypeKind::Custom(p_name, p_args), TypeKind::Custom(a_name, a_args)) => {
                self.infer_inherited_generics(p_name, p_args, a_name, a_args, mapping);
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
        elem: &TypeArgument,
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
        k: &TypeArgument,
        v: &TypeArgument,
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
        elem: &TypeArgument,
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
        elem: &TypeArgument,
        size: &TypeArgument,
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
        p_elems: &[TypeArgument],
        a_elems: &[TypeArgument],
        mapping: &mut HashMap<String, Type>,
    ) {
        for (p_elem_expr, a_elem_expr) in p_elems.iter().zip(a_elems.iter()) {
            if let (Ok(p_elem), Ok(a_elem)) = (
                self.extract_type_from_argument(p_elem_expr),
                self.extract_type_from_argument(a_elem_expr),
            ) {
                self.infer_generic_types(&p_elem, &a_elem, mapping);
            }
        }
    }

    fn infer_custom_generics(
        &self,
        p_args: &Option<Vec<TypeArgument>>,
        a_args: &Option<Vec<TypeArgument>>,
        mapping: &mut HashMap<String, Type>,
    ) {
        if let (Some(p_args), Some(a_args)) = (p_args, a_args) {
            if p_args.len() == a_args.len() {
                for (p_arg_expr, a_arg_expr) in p_args.iter().zip(a_args.iter()) {
                    if let (Ok(p_arg), Ok(a_arg)) = (
                        self.extract_type_from_argument(p_arg_expr),
                        self.extract_type_from_argument(a_arg_expr),
                    ) {
                        self.infer_generic_types(&p_arg, &a_arg, mapping);
                    }
                }
            }
        }
    }

    /// Bind the parameters `p_args` names from the arguments the type
    /// `a_name<a_args>` reaches its supertype `p_name` at. Nothing binds when
    /// `p_name` is not above `a_name`.
    fn infer_inherited_generics(
        &self,
        p_name: &str,
        p_args: &Option<Vec<TypeArgument>>,
        a_name: &str,
        a_args: &Option<Vec<TypeArgument>>,
        mapping: &mut HashMap<String, Type>,
    ) {
        let Some(p_args) = p_args else {
            return;
        };
        if !self.is_subtype(a_name, p_name) {
            return;
        }
        let Some(inherited) = self.supertype_arguments(a_name, a_args.as_deref(), p_name) else {
            return;
        };
        for (p_arg_expr, inherited) in p_args.iter().zip(inherited) {
            if let (Ok(p_arg), Some(inherited)) =
                (self.extract_type_from_argument(p_arg_expr), inherited)
            {
                self.infer_generic_types(&p_arg, &inherited, mapping);
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
        args: &Option<Vec<TypeArgument>>,
        mapping: &HashMap<String, Type>,
    ) -> Type {
        if args.is_none() {
            if let Some(subst) = mapping.get(name) {
                // Bare value-generic reference (e.g. `Size` parsed as a type
                // name inside a position the type checker reaches): pull the
                // original value expression out of the marker so the caller
                // doesn't see the synthetic `Custom("__value_generic__", …)`.
                if let Some(value_expr) = extract_value_generic(subst) {
                    if let Ok(value_ty) = self.extract_type_from_argument(value_expr) {
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
                    if self.extract_type_from_argument(arg).is_ok() {
                        let arg_type = self
                            .extract_type_from_argument(arg)
                            .unwrap_or(make_type(TypeKind::Error));
                        let subst_arg = self.substitute_type(&arg_type, mapping);
                        self.create_type_argument(subst_arg)
                    } else {
                        // Value-generic position (e.g. the `Size` slot in a
                        // resolved `Custom("Array", [..., Identifier("Size")])`).
                        // Rewrite identifier references through the value-generic
                        // marker mapping and keep the expression verbatim
                        // otherwise.
                        substitute_value_generic(arg, mapping)
                    }
                })
                .collect()
        });

        make_type(TypeKind::Custom(name.to_string(), new_args))
    }

    fn substitute_list(&self, elem_expr: &TypeArgument, mapping: &HashMap<String, Type>) -> Type {
        let elem = self
            .extract_type_from_argument(elem_expr)
            .unwrap_or(make_type(TypeKind::Error));
        let subst_elem = self.substitute_type(&elem, mapping);
        make_type(TypeKind::Custom(
            BuiltinCollectionKind::List.name().to_string(),
            Some(vec![self.create_type_argument(subst_elem)]),
        ))
    }

    fn substitute_map(
        &self,
        k_expr: &TypeArgument,
        v_expr: &TypeArgument,
        mapping: &HashMap<String, Type>,
    ) -> Type {
        let k = self
            .extract_type_from_argument(k_expr)
            .unwrap_or(make_type(TypeKind::Error));
        let v = self
            .extract_type_from_argument(v_expr)
            .unwrap_or(make_type(TypeKind::Error));
        let subst_k = self.substitute_type(&k, mapping);
        let subst_v = self.substitute_type(&v, mapping);
        make_type(TypeKind::Custom(
            BuiltinCollectionKind::Map.name().to_string(),
            Some(vec![
                self.create_type_argument(subst_k),
                self.create_type_argument(subst_v),
            ]),
        ))
    }

    fn substitute_set(&self, elem_expr: &TypeArgument, mapping: &HashMap<String, Type>) -> Type {
        let elem = self
            .extract_type_from_argument(elem_expr)
            .unwrap_or(make_type(TypeKind::Error));
        let subst_elem = self.substitute_type(&elem, mapping);
        make_type(TypeKind::Custom(
            BuiltinCollectionKind::Set.name().to_string(),
            Some(vec![self.create_type_argument(subst_elem)]),
        ))
    }

    fn substitute_array(
        &self,
        elem_expr: &TypeArgument,
        size_expr: &TypeArgument,
        mapping: &HashMap<String, Type>,
    ) -> Type {
        let elem = self
            .extract_type_from_argument(elem_expr)
            .unwrap_or(make_type(TypeKind::Error));
        let subst_elem = self.substitute_type(&elem, mapping);
        let subst_size = substitute_value_generic(size_expr, mapping);
        make_type(TypeKind::Custom(
            BuiltinCollectionKind::Array.name().to_string(),
            Some(vec![self.create_type_argument(subst_elem), subst_size]),
        ))
    }

    fn substitute_option(&self, inner: &Type, mapping: &HashMap<String, Type>) -> Type {
        make_type(TypeKind::Option(Box::new(
            self.substitute_type(inner, mapping),
        )))
    }

    fn substitute_tuple(&self, elements: &[TypeArgument], mapping: &HashMap<String, Type>) -> Type {
        let new_elements = elements
            .iter()
            .map(|elem_expr| {
                let elem = self
                    .extract_type_from_argument(elem_expr)
                    .unwrap_or(make_type(TypeKind::Error));
                let subst = self.substitute_type(&elem, mapping);
                self.create_type_argument(subst)
            })
            .collect();
        make_type(TypeKind::Tuple(new_elements))
    }

    fn substitute_result(
        &self,
        ok_expr: &TypeArgument,
        err_expr: &TypeArgument,
        mapping: &HashMap<String, Type>,
    ) -> Type {
        // The substituted type is spelled as the `Result` enum, as
        // `resolve_type_kind` spells a written one: the native kind carries no
        // drop, layout or payload reads past the type checker.
        let (ok, err) = if let (Ok(ok), Ok(err)) = (
            self.extract_type_from_argument(ok_expr),
            self.extract_type_from_argument(err_expr),
        ) {
            (
                self.create_type_argument(self.substitute_type(&ok, mapping)),
                self.create_type_argument(self.substitute_type(&err, mapping)),
            )
        } else {
            (ok_expr.clone(), err_expr.clone())
        };
        make_type(TypeKind::Custom(
            crate::ast::types::RESULT_TYPE_NAME.to_string(),
            Some(vec![ok, err]),
        ))
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
    ///
    /// Returns whether as many arguments were written as there are
    /// parameters: when not, the count is reported and no argument is read
    /// against a parameter, so the caller has no instantiation to build.
    pub(crate) fn validate_generics(
        &mut self,
        args: &Option<Vec<TypeArgument>>,
        params: &Option<Vec<GenericDefinition>>,
        context: &Context,
        span: Span,
    ) -> bool {
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
            return false;
        }

        if let (Some(args_vec), Some(params_vec)) = (args, params) {
            for (i, arg_expr) in args_vec.iter().enumerate() {
                // Skip value-generic args (e.g. the `3` in `Foo<float, 3>`):
                // they aren't type expressions, so `resolve_type_expression`
                // would report "Expected type expression". Constraints on
                // value generics aren't modeled today; trying to validate one
                // as if it were a type produces a spurious error at every
                // instantiation site.
                if self.extract_type_from_argument(arg_expr).is_err() {
                    continue;
                }
                let param_def = &params_vec[i];
                let arg_type = self.resolve_type_argument(arg_expr, context);

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
        true
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
                    .map(|c| self.resolve_bound_type(c, context));

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
/// The type arguments `target` and `source` hold at the same positions, when
/// both are built by the same constructor with the same arity; none otherwise.
fn argument_pairs<'t>(
    target: &'t Type,
    source: &'t Type,
) -> Vec<(&'t TypeArgument, &'t TypeArgument)> {
    if let (TypeKind::Custom(name, Some(args)), TypeKind::Custom(other, Some(other_args))) =
        (&target.kind, &source.kind)
    {
        if name == other && args.len() == other_args.len() {
            return args.iter().zip(other_args).collect();
        }
    } else if let (TypeKind::Tuple(parts), TypeKind::Tuple(other_parts)) =
        (&target.kind, &source.kind)
    {
        if parts.len() == other_parts.len() {
            return parts.iter().zip(other_parts).collect();
        }
    } else if let (TypeKind::List(element), TypeKind::List(other))
    | (TypeKind::Set(element), TypeKind::Set(other))
    | (TypeKind::Future(element), TypeKind::Future(other)) = (&target.kind, &source.kind)
    {
        return vec![(&**element, &**other)];
    } else if let (TypeKind::Map(key, value), TypeKind::Map(other_key, other_value))
    | (TypeKind::Result(key, value), TypeKind::Result(other_key, other_value)) =
        (&target.kind, &source.kind)
    {
        return vec![(&**key, &**other_key), (&**value, &**other_value)];
    }
    Vec::new()
}

impl TypeChecker {
    /// The inference slots of `target` that `source` binds.
    ///
    /// A variant or empty-collection constructor binds only the parameters its
    /// arguments name, so `E.L("s")` is an `E<String, B>` and `List([E.L("s")])`
    /// a `List<E<String, B>>`. `B` is a slot: a parameter no enclosing
    /// declaration declares, standing for a type nothing chose. The value is
    /// laid out without it, so a value that does bind it — an
    /// `E<String, i128>`, whose payload slots are wider — cannot be stored
    /// where the slot is: a read or release through the location's type would
    /// land at the wrong offset.
    ///
    /// A parameter the enclosing body declares is not a slot; it stands for
    /// the body's own argument, which every value there shares.
    pub(crate) fn inference_slots_bound_by(
        &self,
        target: &Type,
        source: &Type,
        context: &Context,
    ) -> Vec<String> {
        self.inference_slot_bindings(target, source, context)
            .into_iter()
            .map(|(slot, _)| slot)
            .collect()
    }

    /// Each inference slot of `target` that `source` binds, with the type
    /// `source` binds it to (see [`Self::inference_slots_bound_by`]).
    pub(crate) fn inference_slot_bindings(
        &self,
        target: &Type,
        source: &Type,
        context: &Context,
    ) -> Vec<(String, Type)> {
        let mut bound = Vec::new();
        self.collect_slots_bound_by(target, source, context, &mut bound);
        bound
    }

    fn collect_slots_bound_by(
        &self,
        target: &Type,
        source: &Type,
        context: &Context,
        bound: &mut Vec<(String, Type)>,
    ) {
        // A type already reported as wrong binds nothing: a second diagnostic
        // about it would only repeat the first.
        if matches!(target.kind, TypeKind::Error) || matches!(source.kind, TypeKind::Error) {
            return;
        }
        if let TypeKind::Generic(name, _, _) = &target.kind {
            let is_declared = matches!(
                context.resolve_type_definition(name),
                Some(TypeDefinition::Generic(_))
            );
            // A slot filled by another open parameter is still unbound.
            let source_is_open = matches!(source.kind, TypeKind::Generic(..));
            if !is_declared && !source_is_open && !bound.iter().any(|(slot, _)| slot == name) {
                bound.push((name.clone(), source.clone()));
            }
            return;
        }
        if let (TypeKind::Option(inner), TypeKind::Option(other))
        | (TypeKind::Linear(inner), TypeKind::Linear(other)) = (&target.kind, &source.kind)
        {
            self.collect_slots_bound_by(inner, other, context, bound);
            return;
        }
        // Any other pairing names no arguments to compare, or is two different
        // types, which the compatibility check reports on its own.
        let pairs = argument_pairs(target, source);
        for (target_arg, source_arg) in pairs {
            if let (TypeArgKind::Type(target_ty, _), TypeArgKind::Type(source_ty, _)) =
                (&target_arg.node, &source_arg.node)
            {
                self.collect_slots_bound_by(target_ty, source_ty, context, bound);
            }
        }
    }

    /// Every inference slot `ty` holds anywhere inside it (see
    /// [`Self::inference_slots_bound_by`]).
    pub(crate) fn inference_slots_of(&self, ty: &Type, context: &Context) -> Vec<String> {
        let mut slots = Vec::new();
        Self::collect_inference_slots(ty, context, &mut slots);
        slots
    }

    fn collect_inference_slots(ty: &Type, context: &Context, slots: &mut Vec<String>) {
        let mut argument = |arg: &TypeArgument| {
            if let TypeArgKind::Type(inner, _) = &arg.node {
                Self::collect_inference_slots(inner, context, slots);
            }
        };
        match &ty.kind {
            TypeKind::Generic(name, _, _) => {
                let declared = matches!(
                    context.resolve_type_definition(name),
                    Some(TypeDefinition::Generic(_))
                );
                if !declared && !slots.contains(name) {
                    slots.push(name.clone());
                }
            }
            TypeKind::Option(inner) | TypeKind::Linear(inner) => {
                Self::collect_inference_slots(inner, context, slots)
            }
            TypeKind::List(elem) | TypeKind::Set(elem) | TypeKind::Future(elem) => argument(elem),
            TypeKind::Array(elem, _) => argument(elem),
            TypeKind::Map(first, second) | TypeKind::Result(first, second) => {
                argument(first);
                argument(second);
            }
            TypeKind::Tuple(elements) | TypeKind::OneOf(elements) => {
                elements.iter().for_each(argument)
            }
            TypeKind::Custom(_, Some(args)) => args.iter().for_each(argument),
            // A type already reported as wrong holds no slot to report again.
            TypeKind::Error => {}
            // A type names a type rather than holding a value of it, a function
            // value's parameters are fixed where it is written, and every other
            // type carries no argument to leave open.
            TypeKind::Meta(_)
            | TypeKind::Function(_)
            | TypeKind::Custom(_, None)
            | TypeKind::Int
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
            | TypeKind::Float
            | TypeKind::F16
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::String
            | TypeKind::Boolean
            | TypeKind::Identifier
            | TypeKind::RawPtr
            | TypeKind::Void => {}
        }
    }

    /// Report a store of a `source` value into a location typed `target`
    /// whose inference slots `source` binds (see
    /// [`Self::inference_slots_bound_by`]). Returns whether it was reported.
    pub(crate) fn refuse_binding_an_inference_slot(
        &mut self,
        target: &Type,
        source: &Type,
        what: &str,
        span: Span,
        context: &Context,
    ) -> bool {
        let bound = self.inference_slots_bound_by(target, source, context);
        let Some(first) = bound.first() else {
            return false;
        };
        self.report_error(
            DiagnosticCode::TypTypeInference,
            format!(
                "Cannot store {source} into {what} of type {target}: that type leaves `{first}` \
                 unbound, because the value it was inferred from never chose it, and {source} \
                 binds it at a different layout. Declare the type where it is first written, \
                 with `{first}` filled in"
            ),
            span,
        );
        true
    }
}

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
    use crate::ast::type_arguments::value_argument;
    use crate::ast::IdNode;

    fn plus(left: ValueArg, right: ValueArg) -> TypeArgument {
        IdNode::new(
            0,
            TypeArgKind::Value(ValueArg::Binary(
                ArithOp::Add,
                Box::new(left),
                Box::new(right),
            )),
            Span::new(0, 0),
        )
    }

    fn name(name: &str) -> ValueArg {
        ValueArg::Name(name.to_string())
    }

    fn size_bound_to(value: i128) -> HashMap<String, Type> {
        HashMap::from([(
            "Size".to_string(),
            value_generic_marker_type(value_argument(value)),
        )])
    }

    /// An operand bound to a value folds; one the body declares but leaves
    /// unbound is not yet a value; one the body does not declare is never one.
    #[test]
    fn only_a_declared_parameter_keeps_a_value_argument_open() {
        let bound = size_bound_to(2);
        let open = HashSet::from(["Rest".to_string()]);
        assert_eq!(
            unfoldable_value_argument(&plus(name("Size"), ValueArg::Int(1)), &bound, &open),
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
