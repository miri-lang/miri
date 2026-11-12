// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The bounds on the class instantiations a program may need, and the shape of
//! a type they are measured over.
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
//!   [`MAX_INSTANCE_TYPE_DEPTH`] type constructors deep,
//! - an instance whose type arguments are built from more than
//!   [`MAX_INSTANCE_TYPE_SIZE`] types, which a type doubling on every level
//!   passes long before it nests past the depth bound, and
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
//!
//! The bounds, the measures they are read with and the refusal that names a
//! passed bound are stated here once, below MIR lowering, so every stage that
//! decides which instances a program needs applies the same bounds and spells
//! the same refusal. The drop-instantiation closure the backend reads bounds
//! itself by the same measure, so a type one side refuses is refused by the
//! other.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::type_arguments::{argument_type_marked, TypeArgument};
use crate::ast::types::{BuiltinCollectionKind, FunctionTypeData, Type, TypeKind, TUPLE_TYPE_NAME};
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::generics::{
    extract_value_generic_kind, value_generic_marker_type, value_generic_slot,
};
use std::borrow::Cow;
use std::collections::HashMap;

/// How many type constructors deep one instance may nest, counting the class
/// itself: `Vec<List<List<String>>>` is three deep.
///
/// Held under the symbol mangler's own depth, so every instance within it has
/// a per-instantiation name, and a body is never shared across instances
/// because the mangler ran out of depth.
pub const MAX_INSTANCE_TYPE_DEPTH: usize = 32;

/// How many types the arguments of one instance may be built from, all told,
/// each counted once per place it is written: `(int, (int, String))` is five.
///
/// The depth bound alone admits a type that doubles on every level, `(T, T)`
/// inside a body at `T`, until it holds 2^[`MAX_INSTANCE_TYPE_DEPTH`] types;
/// every instance on the way is lowered, spelled and hashed at its full size,
/// so such a program would be refused only after exponential work. This bound
/// is far above any type a program writes, and well below the size at which
/// lowering one instance becomes slow.
pub const MAX_INSTANCE_TYPE_SIZE: usize = 1024;

/// How many instantiations at value arguments one class may need.
pub const MAX_VALUE_INSTANCES_PER_CLASS: usize = 256;

/// How many type constructors deep an instance spelled in a diagnostic's
/// headline is shown before the rest is elided.
pub const SPELLED_LEVELS: usize = 3;

/// How many type constructors deep each step of a growth chain is shown.
pub const CHAIN_LEVELS: usize = 4;

/// How many steps of a growth chain a diagnostic shows before eliding the
/// rest.
const CHAIN_STEPS: usize = 3;

/// The help for a type argument that grows through a trait call.
pub const DEPTH_THROUGH_TRAIT_HELP: &str = "the type argument grows on every call through the trait; bound the recursion by type, or keep one type (e.g. store the depth as a value instead of in the type)";

/// The help for a type argument that grows through a static call.
pub const DEPTH_HELP: &str = "the type argument grows on every call; bound the recursion by type, or keep one type (e.g. store the depth as a value instead of in the type)";

/// The help for a value argument that changes on every call.
pub const VALUE_HELP: &str = "the value argument changes on every call; bound the recursion, or keep one value (e.g. store the size in a field instead of in the type)";

/// One argument of a generic-class reference as the instantiation registry
/// records it and every per-instantiation symbol is mangled from: the type it
/// names, a nullable mark read as the option it denotes, or the marker
/// standing for a value generic. `None` when the argument is neither.
///
/// A static call, a vtable and a drop thunk all name an instantiation from
/// what this answers, so they cannot spell one instantiation two ways.
pub fn instantiation_argument(arg: &TypeArgument) -> Option<Type> {
    argument_type_marked(arg).or_else(|| value_generic_slot(arg))
}

/// The type a closure type's parameter or return is written as, a nullable
/// mark read as the option it denotes; `None` for anything else.
pub fn written_component_type(expr: &Expression) -> Option<Type> {
    let ExpressionKind::Type(ty, nullable) = &expr.node else {
        return None;
    };
    Some(if *nullable {
        Type::new(TypeKind::Option(ty.clone()), ty.span)
    } else {
        (**ty).clone()
    })
}

/// The outermost constructor of a type, as [`constructor_parts`] reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Constructor<'t> {
    /// A constructor spelled by name: a declared type's, a type parameter's,
    /// or a built-in constructor's, which starts with a digit no declared
    /// type's name can start with, so a class named `tuple` is never read as
    /// one.
    Named(&'t str),
    /// A type built from nothing, or a value generic's size, spelled by the
    /// token the symbol mangler gives its kind.
    Leaf(&'t TypeKind),
}

/// `ty` as its outermost constructor and the types it is built from, in
/// order. A value generic's size and every type built from nothing are
/// leaves.
pub fn constructor_parts(ty: &Type) -> (Constructor<'_>, Vec<Type>) {
    let leaf = || (Constructor::Leaf(&ty.kind), Vec::new());
    if extract_value_generic_kind(&ty.kind).is_some() {
        return leaf();
    }
    let arguments = |args: &[&TypeArgument]| -> Vec<Type> {
        args.iter()
            .filter_map(|arg| instantiation_argument(arg))
            .collect()
    };
    match &ty.kind {
        TypeKind::Custom(name, args) => {
            let args: Vec<&TypeArgument> = args.iter().flatten().collect();
            (Constructor::Named(name), arguments(&args))
        }
        TypeKind::Generic(name, _) => (Constructor::Named(name), Vec::new()),
        TypeKind::Option(inner) => (Constructor::Named("0option"), vec![(**inner).clone()]),
        TypeKind::Linear(inner) => (Constructor::Named("0linear"), vec![(**inner).clone()]),
        TypeKind::Meta(inner) => (Constructor::Named("0meta"), vec![(**inner).clone()]),
        TypeKind::Tuple(elements) => {
            let elements: Vec<&TypeArgument> = elements.iter().collect();
            (Constructor::Named("0tuple"), arguments(&elements))
        }
        TypeKind::OneOf(members) => {
            let members: Vec<&TypeArgument> = members.iter().collect();
            (Constructor::Named("0oneof"), arguments(&members))
        }
        TypeKind::List(element) => (Constructor::Named("0list"), arguments(&[element])),
        TypeKind::Set(element) => (Constructor::Named("0set"), arguments(&[element])),
        TypeKind::Future(element) => (Constructor::Named("0future"), arguments(&[element])),
        TypeKind::Array(element, size) => {
            (Constructor::Named("0array"), arguments(&[element, size]))
        }
        TypeKind::Map(key, value) => (Constructor::Named("0map"), arguments(&[key, value])),
        TypeKind::Result(ok, err) => (Constructor::Named("0result"), arguments(&[ok, err])),
        TypeKind::Function(function) => {
            let parts = function.params.iter().map(|param| &*param.typ);
            let parts = parts.chain(function.return_type.as_deref());
            (
                Constructor::Named("0fn"),
                parts.filter_map(written_component_type).collect(),
            )
        }
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
        | TypeKind::Error => leaf(),
    }
}

/// How many types `args` are built from, each counted once per place it is
/// written, stopping one past [`MAX_INSTANCE_TYPE_SIZE`].
pub fn arguments_type_size<'t>(args: impl IntoIterator<Item = &'t Type>) -> usize {
    let mut remaining = MAX_INSTANCE_TYPE_SIZE + 1;
    for arg in args {
        count_types(arg, &mut remaining);
    }
    MAX_INSTANCE_TYPE_SIZE + 1 - remaining
}

/// Count `ty` and every type it is built from against `remaining`, stopping
/// once it runs out.
fn count_types(ty: &Type, remaining: &mut usize) {
    let Some(rest) = remaining.checked_sub(1) else {
        return;
    };
    *remaining = rest;
    for part in constructor_parts(ty).1 {
        count_types(&part, remaining);
    }
}

/// How many type constructors deep an instance at `args` nests, counting its
/// class: one more than its deepest argument. Past
/// [`MAX_INSTANCE_TYPE_DEPTH`] the count stops one level beyond it, which is
/// all a caller asking whether the bound is passed needs.
pub fn instance_type_depth(args: &[Type]) -> usize {
    1 + deepest_argument_depth(args)
}

/// How many type constructors deep the deepest of `args` nests, stopping one
/// level past [`MAX_INSTANCE_TYPE_DEPTH`].
pub fn deepest_argument_depth<'t>(args: impl IntoIterator<Item = &'t Type>) -> usize {
    args.into_iter()
        .map(|arg| type_depth(arg, MAX_INSTANCE_TYPE_DEPTH))
        .max()
        .unwrap_or(0)
}

/// How many type constructors deep `ty` nests, a type built from nothing
/// counting none, looking no further than `budget` levels down.
fn type_depth(ty: &Type, budget: usize) -> usize {
    let parts = constructor_parts(ty).1;
    if parts.is_empty() {
        return 0;
    }
    match budget.checked_sub(1) {
        Some(rest) => {
            1 + parts
                .iter()
                .map(|part| type_depth(part, rest))
                .max()
                .unwrap_or(0)
        }
        None => 1,
    }
}

/// The bound an instance passes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExceededLimit {
    /// The instance nests this many type constructors deep.
    TypeDepth(usize),
    /// The instance's type arguments are built from more than
    /// [`MAX_INSTANCE_TYPE_SIZE`] types.
    TypeSize,
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

/// Whether an instance of `class` at `args` takes a value that a program can
/// compute afresh on every call, and so counts toward its class's bound.
///
/// The tuple class's value is a tuple type's length, which only the program's
/// text writes: substituting a type for a parameter replaces an element, never
/// adds one, so a program reaches no more lengths than it spells.
fn grows_by_value(class: &str, args: &[Type]) -> bool {
    class != TUPLE_TYPE_NAME && has_value_argument(args)
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
    if arguments_type_size(args) > MAX_INSTANCE_TYPE_SIZE {
        return Some(ExceededLimit::TypeSize);
    }
    if value_instances > MAX_VALUE_INSTANCES_PER_CLASS && grows_by_value(class, args) {
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
///
/// TODO: two programs with no recursion reach this refusal. A `Set<(E, int)>`
/// whose enum `E` carries a tuple payload is refused 33 levels deep, and so is
/// a generic parameter that shadows a declared type of the same name
/// (`struct T` beside `fn pairs<T>` holding a `Set<(T, String)>`). Some walk
/// feeding the depth count treats a type that holds a tuple, or a parameter
/// spelled like a declared type, as growing.
pub fn polymorphic_recursion(
    class: &str,
    args: &[Type],
    limit: &ExceededLimit,
    growth: &Growth,
    span: Span,
) -> LoweringError {
    let refusal = growth_refusal(class, args, limit, growth);
    let error = LoweringError::coded(
        DiagnosticCode::MirPolymorphicRecursion,
        refusal.message,
        span,
        Some(refusal.help.to_string()),
    );
    match refusal.note {
        Some(note) => error.with_note(note),
        None => error,
    }
}

/// What the refusal of an instance past a bound says, whichever stage
/// reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrowthRefusal {
    pub message: String,
    pub help: &'static str,
    /// The steps the growth took to the instance, when an earlier instance of
    /// its class led to it.
    pub note: Option<String>,
}

/// What the refusal of an instance of `class` at `args` that passes `limit`
/// says, explained by how `growth` reached it.
pub fn growth_refusal(
    class: &str,
    args: &[Type],
    limit: &ExceededLimit,
    growth: &Growth,
) -> GrowthRefusal {
    let instance = spelled_instance(class, args, SPELLED_LEVELS);
    let type_growth_help = if growth.through_trait {
        DEPTH_THROUGH_TRAIT_HELP
    } else {
        DEPTH_HELP
    };
    let (message, help) = match limit {
        ExceededLimit::TypeDepth(depth) => (
            format!("instantiating `{instance}` nests its type argument {depth} levels deep"),
            type_growth_help,
        ),
        ExceededLimit::TypeSize => (
            format!(
                "instantiating `{instance}` builds its type argument from more types than one \
                 instance may hold ({MAX_INSTANCE_TYPE_SIZE})"
            ),
            type_growth_help,
        ),
        ExceededLimit::ValueInstances { parameter } => (
            format!(
                "instantiating `{instance}`: `{class}` needs more than \
                 {MAX_VALUE_INSTANCES_PER_CLASS} instantiations of `{parameter}`"
            ),
            VALUE_HELP,
        ),
    };
    GrowthRefusal {
        message,
        help,
        note: growth_note(class, args, limit, growth),
    }
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
        ExceededLimit::TypeDepth(_) | ExceededLimit::TypeSize => "a larger type".to_string(),
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
        out.push_str(&value.node.to_string());
        return;
    }
    match kind {
        TypeKind::Custom(name, None) | TypeKind::Generic(name, _) => out.push_str(name),
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
        | TypeKind::OneOf(_)
        | TypeKind::Error => out.push_str(&kind.to_string()),
    }
}

/// Append `head` applied to the type arguments `parts` write.
fn push_applied<'e>(
    out: &mut String,
    head: &str,
    parts: impl IntoIterator<Item = &'e TypeArgument>,
    levels: usize,
) {
    push_angled(out, head, parts.into_iter().map(argument_type), levels);
}

/// Append a function type, `fn(int, String) bool`.
fn push_function(out: &mut String, function: &FunctionTypeData, levels: usize) {
    let params = function
        .params
        .iter()
        .map(|param| component_type(&param.typ));
    out.push_str("fn");
    push_listed(out, ("(", ")"), params, levels);
    if let Some(ret) = &function.return_type {
        out.push(' ');
        push_type(out, &component_type(ret).kind, levels.saturating_sub(1));
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
/// value argument's marker; a value not yet folded is spelled as written.
fn argument_type(arg: &TypeArgument) -> Cow<'static, Type> {
    Cow::Owned(
        instantiation_argument(arg).unwrap_or_else(|| value_generic_marker_type(arg.clone())),
    )
}

/// The type a closure type's parameter or return is written as.
fn component_type(written: &Expression) -> Cow<'static, Type> {
    Cow::Owned(
        written_component_type(written).unwrap_or_else(|| Type::new(TypeKind::Error, written.span)),
    )
}
