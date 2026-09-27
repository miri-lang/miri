// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The bounds on the class instantiations a program may need.
//!
//! A generic class is compiled once per set of arguments a program reaches it
//! at. A method that builds its own class at an argument grown from its own —
//! `Impl<Wrap<T>>` inside `Impl<T>`, `Buf<T, Size + 1>` inside `Buf<T, Size>`
//! — and reaches that instance's methods again asks for a new instantiation on
//! every call, and no finite set of compiled bodies covers it. Whether the
//! growth ever stops is a question about the program's run, which the compiler
//! cannot answer, so it bounds each instantiation instead and refuses a program
//! that needs one past a bound:
//!
//! - an instance whose type arguments nest more than
//!   [`MAX_INSTANCE_TYPE_DEPTH`] type constructors deep, and
//! - a class needing more than [`MAX_VALUE_INSTANCES_PER_CLASS`]
//!   instantiations at value arguments.
//!
//! The first is a property of one instance, the second of how many instances
//! one class has; neither depends on the order the instances are found in, so
//! a program is refused exactly when the set of instances it needs holds one
//! past a bound. The bounds stop the two ways an instantiation grows — a type
//! nested deeper, a value computed afresh — but are not a proof that every
//! program within them needs few instances: two arguments growing side by
//! side reach every combination up to the depth bound before either passes it.

use super::instantiation_argument;
use crate::ast::expression::Expression;
use crate::ast::formatter::expression_text;
use crate::ast::types::{BuiltinCollectionKind, FunctionTypeData, Type, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
pub(crate) use crate::mir::instantiation::shape::constructor_parts;
pub use crate::mir::instantiation::shape::{
    deepest_argument_depth, instance_type_depth, mentions_open_parameter, MAX_INSTANCE_TYPE_DEPTH,
};
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::generics::{extract_value_generic_kind, UnfoldableValue};
use std::borrow::Cow;
use std::collections::HashMap;

/// How many instantiations at value arguments one class may need.
pub const MAX_VALUE_INSTANCES_PER_CLASS: usize = 256;

/// How many type constructors deep an instance spelled in a diagnostic's
/// headline is shown before the rest is elided.
const SPELLED_LEVELS: usize = 3;

/// How many type constructors deep each step of a growth chain is shown.
const CHAIN_LEVELS: usize = 4;

/// How many steps of a growth chain a diagnostic shows before eliding the
/// rest.
const CHAIN_STEPS: usize = 3;

/// The help for a type argument that grows through a trait call.
pub const DEPTH_THROUGH_TRAIT_HELP: &str = "the type argument grows on every call through the trait; bound the recursion by type, or keep one type (e.g. store the depth as a value instead of in the type)";

/// The help for a type argument that grows through a static call.
pub const DEPTH_HELP: &str = "the type argument grows on every call; bound the recursion by type, or keep one type (e.g. store the depth as a value instead of in the type)";

/// The help for a value argument that changes on every call.
pub const VALUE_HELP: &str = "the value argument changes on every call; bound the recursion, or keep one value (e.g. store the size in a field instead of in the type)";

/// The help for a value argument that has no value at its instantiation.
pub const VALUE_ARGUMENT_HELP: &str = "a value argument must fold to an integer within the signed 128-bit range; bound the value, or keep it in a field instead of in the type";

/// The help for a type argument the compiler has no name for.
pub const TYPE_ARGUMENT_HELP: &str = "instantiate the class at a type the compiler can name; wrap the value in a class or struct and instantiate at that";

/// The bound an instance passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExceededLimit {
    /// The instance nests this many type constructors deep.
    TypeDepth(usize),
    /// The instance's class needs more instantiations at value arguments than
    /// [`MAX_VALUE_INSTANCES_PER_CLASS`]; `parameter` is the one it takes a
    /// value for.
    ValueInstances { parameter: String },
}

/// Whether an instance at `args` takes a value for some parameter.
pub fn has_value_argument(args: &[Type]) -> bool {
    args.iter()
        .any(|arg| extract_value_generic_kind(&arg.kind).is_some())
}

/// The bound an instance of `class` at `args` passes, if it passes one.
///
/// `value_instances` counts the distinct instantiations of `class` at value
/// arguments the program needs, this one included when it is one.
pub fn exceeded_limit(
    class: &str,
    args: &[Type],
    value_instances: usize,
    type_defs: &HashMap<String, TypeDefinition>,
) -> Option<ExceededLimit> {
    let depth = instance_type_depth(args);
    if depth > MAX_INSTANCE_TYPE_DEPTH {
        return Some(ExceededLimit::TypeDepth(depth));
    }
    if value_instances > MAX_VALUE_INSTANCES_PER_CLASS && has_value_argument(args) {
        return Some(ExceededLimit::ValueInstances {
            parameter: value_parameter(class, args, type_defs),
        });
    }
    None
}

/// The name of the first parameter of `class` that `args` fill with a value.
fn value_parameter(
    class: &str,
    args: &[Type],
    type_defs: &HashMap<String, TypeDefinition>,
) -> String {
    let declared = type_defs
        .get(class)
        .and_then(TypeDefinition::generics)
        .unwrap_or_default();
    declared
        .iter()
        .zip(args)
        .find(|(_, arg)| extract_value_generic_kind(&arg.kind).is_some())
        .map_or_else(String::new, |(param, _)| param.name.clone())
}

/// How a refused instance was reached: the earlier instances of its class on
/// the way to it, outermost first, and the method whose body built each next
/// one.
#[derive(Debug, Clone, Default)]
pub struct Growth<'a> {
    pub chain: Vec<&'a [Type]>,
    pub method: Option<&'a str>,
    pub through_trait: bool,
}

/// The refusal of an instance of `class` at `args`, built at `span`, that
/// passes `limit`, explained by how `growth` reached it.
pub fn polymorphic_recursion(
    class: &str,
    args: &[Type],
    limit: &ExceededLimit,
    growth: &Growth,
    span: Span,
) -> LoweringError {
    let instance = spelled_instance(class, args, SPELLED_LEVELS);
    let (message, help) = match limit {
        ExceededLimit::TypeDepth(depth) => (
            format!("instantiating `{instance}` nests its type argument {depth} levels deep"),
            if growth.through_trait {
                DEPTH_THROUGH_TRAIT_HELP
            } else {
                DEPTH_HELP
            },
        ),
        ExceededLimit::ValueInstances { parameter } => (
            format!(
                "instantiating `{instance}`: `{class}` needs more than \
                 {MAX_VALUE_INSTANCES_PER_CLASS} instantiations of `{parameter}`"
            ),
            VALUE_HELP,
        ),
    };
    let error = LoweringError::coded(
        DiagnosticCode::MirPolymorphicRecursion,
        message,
        span,
        Some(help.to_string()),
    );
    match growth_note(class, args, limit, growth) {
        Some(note) => error.with_note(note),
        None => error,
    }
}

/// The refusal of an instance of `class` at `args`, built at `span`, whose
/// value argument `argument` has no value once each of `bindings` — a
/// parameter it names and the value that parameter is bound to — is
/// substituted, for the reason `cause`.
pub(crate) fn invalid_value_argument(
    class: &str,
    args: &[Type],
    argument: &Expression,
    bindings: &[(&str, &Expression)],
    cause: UnfoldableValue,
    span: Span,
) -> LoweringError {
    let instance = spelled_instance(class, args, SPELLED_LEVELS);
    let bound: Vec<String> = bindings
        .iter()
        .map(|(name, value)| format!("{name} = {}", expression_text(value)))
        .collect();
    let why = match cause {
        UnfoldableValue::NotConstant => "is not a compile-time constant",
        UnfoldableValue::OutOfRange => "does not fit in a 128-bit integer",
        UnfoldableValue::DivisionByZero => "divides by zero",
        UnfoldableValue::UnsupportedOperator => "uses an operator a value argument cannot fold",
    };
    LoweringError::coded(
        DiagnosticCode::MirInvalidInstantiationArgument,
        format!(
            "instantiating `{instance}` at `{}`: `{}` {why}",
            bound.join(", "),
            expression_text(argument)
        ),
        span,
        Some(VALUE_ARGUMENT_HELP.to_string()),
    )
}

/// The refusal of an instance of `class` at `args`, built at `span`, whose
/// type argument `argument` has no name the compiler can compile a body at.
pub fn unnameable_type_argument(
    class: &str,
    args: &[Type],
    argument: &Type,
    span: Span,
) -> LoweringError {
    let instance = spelled_instance(class, args, SPELLED_LEVELS);
    LoweringError::coded(
        DiagnosticCode::MirInvalidInstantiationArgument,
        format!(
            "instantiating `{instance}`: `{}` has no name the compiler can compile a body at",
            spelled_type(argument, CHAIN_LEVELS)
        ),
        span,
        Some(TYPE_ARGUMENT_HELP.to_string()),
    )
}

/// The note on a value argument of `class` that outgrew the integer range
/// inside a method of `class` itself: each instance builds the next at a new
/// value of `parameter`, and the chain ran out of integers before it reached
/// [`MAX_VALUE_INSTANCES_PER_CLASS`] instantiations.
pub(crate) fn value_growth_note(class: &str, parameter: &str) -> String {
    format!(
        "each instance of `{class}` builds the next at a new value of `{parameter}`, \
         which outgrew a 128-bit integer before `{class}` reached \
         {MAX_VALUE_INSTANCES_PER_CLASS} instantiations; bound the recursion"
    )
}

/// The note naming the steps `growth` took to the instance of `class` at
/// `args`; none when no earlier instance of the class led to it.
fn growth_note(
    class: &str,
    args: &[Type],
    limit: &ExceededLimit,
    growth: &Growth,
) -> Option<String> {
    if growth.chain.is_empty() {
        return None;
    }
    let steps: Vec<&[Type]> = growth.chain.iter().copied().chain([args]).collect();
    let spelled_steps = |levels: usize| -> Vec<String> {
        steps
            .iter()
            .take(CHAIN_STEPS)
            .map(|step| spelled_instance(class, step, levels))
            .collect()
    };
    // Steps nested past the levels shown elide to one spelling; shown in full
    // they differ, since each is a step of growth.
    let mut shown = spelled_steps(CHAIN_LEVELS);
    if shown.windows(2).any(|pair| pair[0] == pair[1]) {
        shown = spelled_steps(MAX_INSTANCE_TYPE_DEPTH + 1);
    }
    if steps.len() > CHAIN_STEPS {
        shown.push("…".to_string());
    }
    let caller = growth.method.map_or_else(
        || "each call".to_string(),
        |m| format!("each call to `{class}.{m}`"),
    );
    let larger = match limit {
        ExceededLimit::TypeDepth(_) => "a larger type".to_string(),
        ExceededLimit::ValueInstances { parameter } => format!("a new value of `{parameter}`"),
    };
    Some(format!(
        "{caller} builds `{class}` at {larger}: {}",
        shown.join(" → ")
    ))
}

/// `class` at `args` as Miri spells it, `Impl<Wrap<int>>`, each argument shown
/// `levels - 1` constructors deep.
pub fn spelled_instance(class: &str, args: &[Type], levels: usize) -> String {
    if args.is_empty() {
        return class.to_string();
    }
    let mut out = String::new();
    push_angled(&mut out, class, args.iter().map(Cow::Borrowed), levels);
    out
}

/// `ty` as Miri spells it, shown `levels` type constructors deep and the rest
/// elided as `…`.
pub fn spelled_type(ty: &Type, levels: usize) -> String {
    let mut out = String::new();
    push_type(&mut out, &ty.kind, levels);
    out
}

/// Append `kind` as Miri spells it, `levels` constructors deep.
fn push_type(out: &mut String, kind: &TypeKind, levels: usize) {
    if let Some(value) = extract_value_generic_kind(kind) {
        out.push_str(&expression_text(value));
        return;
    }
    match kind {
        TypeKind::Custom(name, None) | TypeKind::Generic(name, _, _) => out.push_str(name),
        TypeKind::Custom(name, Some(args)) => push_applied(out, name, args.iter(), levels),
        TypeKind::List(element) => push_applied(
            out,
            BuiltinCollectionKind::List.name(),
            [&**element],
            levels,
        ),
        TypeKind::Set(element) => {
            push_applied(out, BuiltinCollectionKind::Set.name(), [&**element], levels)
        }
        TypeKind::Map(key, value) => {
            let parts = [&**key, &**value];
            push_applied(out, BuiltinCollectionKind::Map.name(), parts, levels)
        }
        TypeKind::Array(element, size) => {
            let parts = [&**element, &**size];
            push_applied(out, BuiltinCollectionKind::Array.name(), parts, levels)
        }
        TypeKind::Result(ok, err) => push_applied(out, "Result", [&**ok, &**err], levels),
        TypeKind::Future(inner) => push_applied(out, "Future", [&**inner], levels),
        TypeKind::Option(inner) => {
            push_type(out, &inner.kind, levels);
            out.push('?');
        }
        TypeKind::Meta(inner) | TypeKind::Linear(inner) => push_type(out, &inner.kind, levels),
        TypeKind::Tuple(elements) => {
            push_listed(out, ("(", ")"), elements.iter().map(argument_type), levels)
        }
        TypeKind::Function(function) => push_function(out, function, levels),
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
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Void
        | TypeKind::Error => out.push_str(&kind.to_string()),
    }
}

/// Append `head` applied to the type arguments `parts` write.
fn push_applied<'e>(
    out: &mut String,
    head: &str,
    parts: impl IntoIterator<Item = &'e Expression>,
    levels: usize,
) {
    push_angled(out, head, parts.into_iter().map(argument_type), levels);
}

/// Append a function type, `fn(int, String) bool`.
fn push_function(out: &mut String, function: &FunctionTypeData, levels: usize) {
    let params = function
        .params
        .iter()
        .map(|param| argument_type(&param.typ));
    out.push_str("fn");
    push_listed(out, ("(", ")"), params, levels);
    if let Some(ret) = &function.return_type {
        out.push(' ');
        push_type(out, &argument_type(ret).kind, levels.saturating_sub(1));
    }
}

/// Append `head<parts>`, or `…` once `levels` runs out.
fn push_angled<'t>(
    out: &mut String,
    head: &str,
    parts: impl Iterator<Item = Cow<'t, Type>>,
    levels: usize,
) {
    if levels == 0 {
        out.push('…');
        return;
    }
    out.push_str(head);
    push_listed(out, ("<", ">"), parts, levels);
}

/// Append `parts` between `brackets`, separated by commas, each `levels - 1`
/// constructors deep; `…` once `levels` runs out.
fn push_listed<'t>(
    out: &mut String,
    (open, close): (&str, &str),
    parts: impl Iterator<Item = Cow<'t, Type>>,
    levels: usize,
) {
    let Some(inner) = levels.checked_sub(1) else {
        out.push('…');
        return;
    };
    out.push_str(open);
    for (index, part) in parts.enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        push_type(out, &part.kind, inner);
    }
    out.push_str(close);
}

/// The type a type-constructor argument stands for: the type it spells, or a
/// value argument's marker; an argument that is neither is spelled as the
/// expression it is.
fn argument_type(arg: &Expression) -> Cow<'static, Type> {
    Cow::Owned(
        instantiation_argument(arg).unwrap_or_else(|| {
            crate::type_checker::generics::value_generic_marker_type(arg.clone())
        }),
    )
}
