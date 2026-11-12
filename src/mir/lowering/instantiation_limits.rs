// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The refusals of an instance lowering cannot compile a body at: a value
//! argument with no value, or a type argument the compiler has no name for.
//!
//! The bounds on the class instantiations a program may need, the measures
//! they are read with and the refusal that names a passed bound are stated in
//! [`crate::type_checker::instantiation_limits`] and re-exported here.

use crate::ast::type_arguments::TypeArgument;
use crate::ast::types::Type;
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
pub(crate) use crate::mir::instantiation::shape::constructor_parts;
pub use crate::mir::instantiation::shape::mentions_open_parameter;
use crate::type_checker::generics::UnfoldableValue;
pub use crate::type_checker::instantiation_limits::{
    arguments_type_size, deepest_argument_depth, exceeded_limit, has_value_argument,
    instance_type_depth, polymorphic_recursion, spelled_instance, spelled_type, ExceededLimit,
    Growth, DEPTH_HELP, DEPTH_THROUGH_TRAIT_HELP, MAX_INSTANCE_TYPE_DEPTH, MAX_INSTANCE_TYPE_SIZE,
    MAX_VALUE_INSTANCES_PER_CLASS, VALUE_HELP,
};
use crate::type_checker::instantiation_limits::{CHAIN_LEVELS, SPELLED_LEVELS};

/// The help for a value argument that has no value at its instantiation.
pub const VALUE_ARGUMENT_HELP: &str = "a value argument must fold to an integer from 1 up to the signed 128-bit maximum; bound the value, or keep it in a field instead of in the type";

/// The help for a type argument the compiler has no name for.
pub const TYPE_ARGUMENT_HELP: &str = "instantiate the class at a type the compiler can name; wrap the value in a class or struct and instantiate at that";

/// The refusal of an instance of `class` at `args`, built at `span`, whose
/// value argument `argument` has no value once each of `bindings` — a
/// parameter it names and the value that parameter is bound to — is
/// substituted, for the reason `cause`.
pub(crate) fn invalid_value_argument(
    class: &str,
    args: &[Type],
    argument: &TypeArgument,
    bindings: &[(&str, &TypeArgument)],
    cause: UnfoldableValue,
    span: Span,
) -> LoweringError {
    let instance = spelled_instance(class, args, SPELLED_LEVELS);
    let bound: Vec<String> = bindings
        .iter()
        .map(|(name, value)| format!("{name} = {}", value.node))
        .collect();
    let why = match cause {
        UnfoldableValue::NotConstant => "is not a compile-time constant",
        UnfoldableValue::OutOfRange => "does not fit in a 128-bit integer",
        UnfoldableValue::DivisionByZero => "divides by zero",
        UnfoldableValue::NotPositive => "is not greater than zero",
    };
    // An argument the substitution already folded names no parameter: the
    // instance it spells is then the whole story.
    let at = if bound.is_empty() {
        String::new()
    } else {
        format!(" at `{}`", bound.join(", "))
    };
    LoweringError::coded(
        DiagnosticCode::MirInvalidInstantiationArgument,
        format!("instantiating `{instance}`{at}: `{}` {why}", argument.node),
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
