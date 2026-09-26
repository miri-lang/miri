// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The bounds on the class instantiations a program may need: how deep one
//! instance nests, and how an instance is spelled when it is refused.

use miri::ast::expression::Expression;
use miri::ast::types::{Type, TypeKind, VALUE_GENERIC_MARKER};
use miri::ast::ExpressionKind;
use miri::error::syntax::Span;
use miri::mir::lowering::instantiation_limits::{
    has_value_argument, instance_type_depth, mentions_open_parameter, polymorphic_recursion,
    spelled_instance, spelled_type, unnameable_type_argument, ExceededLimit, Growth,
    MAX_INSTANCE_TYPE_DEPTH, TYPE_ARGUMENT_HELP,
};
use std::collections::HashMap;

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::new(0, 0))
}

fn argument(kind: TypeKind) -> Expression {
    Expression {
        id: 0,
        span: Span::new(0, 0),
        node: ExpressionKind::Type(Box::new(ty(kind)), false),
    }
}

fn generic(name: &str, args: Vec<TypeKind>) -> TypeKind {
    TypeKind::Custom(
        name.to_string(),
        Some(args.into_iter().map(argument).collect()),
    )
}

fn value(size: i128) -> Type {
    let literal = Expression {
        id: 0,
        span: Span::new(0, 0),
        node: ExpressionKind::Literal(miri::ast::factory::int_literal(size)),
    };
    ty(TypeKind::Custom(
        VALUE_GENERIC_MARKER.to_string(),
        Some(vec![literal]),
    ))
}

fn list_of(kind: TypeKind) -> TypeKind {
    generic("List", vec![kind])
}

/// An instance counts its own class and every constructor its deepest
/// argument nests: `Vec<List<List<String>>>` is three deep.
#[test]
fn an_instance_is_as_deep_as_its_class_and_its_deepest_argument() {
    assert_eq!(instance_type_depth(&[ty(TypeKind::Int)]), 1);
    assert_eq!(
        instance_type_depth(&[ty(list_of(list_of(TypeKind::String)))]),
        3
    );
    assert_eq!(
        instance_type_depth(&[ty(TypeKind::Int), ty(list_of(TypeKind::Int))]),
        2
    );
}

/// A value argument is a leaf, however large the value.
#[test]
fn a_value_argument_adds_no_depth() {
    assert_eq!(instance_type_depth(&[ty(TypeKind::String), value(4096)]), 1);
    assert!(has_value_argument(&[ty(TypeKind::String), value(2)]));
    assert!(!has_value_argument(&[ty(TypeKind::String)]));
}

/// The count stops one level past the bound, so a type nested far deeper
/// costs no deeper a walk.
#[test]
fn the_depth_stops_counting_past_the_bound() {
    let deep = (0..4 * MAX_INSTANCE_TYPE_DEPTH).fold(TypeKind::Int, |inner, _| list_of(inner));
    let depth = instance_type_depth(&[ty(deep)]);
    assert!(depth > MAX_INSTANCE_TYPE_DEPTH, "{depth}");
    assert!(depth <= MAX_INSTANCE_TYPE_DEPTH + 2, "{depth}");
}

/// A refused instance is spelled the way the source writes types, and elided
/// past the levels shown.
#[test]
fn an_instance_is_spelled_in_the_language_s_own_syntax() {
    let list = ty(list_of(TypeKind::String));
    assert_eq!(spelled_type(&list, 4), "List<String>");
    assert_eq!(
        spelled_instance("Buf", &[ty(TypeKind::String), value(2)], 3),
        "Buf<String, 2>"
    );
    let wrapped = (0..5).fold(TypeKind::Int, |inner, _| generic("Wrap", vec![inner]));
    assert_eq!(
        spelled_instance("Impl", &[ty(wrapped)], 3),
        "Impl<Wrap<Wrap<…>>>"
    );
    assert_eq!(
        spelled_type(&ty(TypeKind::Option(Box::new(ty(TypeKind::Int)))), 2),
        "int?"
    );
}

/// A type argument naming a parameter no substitution bound is not an
/// instantiation yet, however deep inside it the parameter sits; one built
/// only from concrete types is.
#[test]
fn an_open_parameter_is_found_anywhere_inside_a_type() {
    let defs = HashMap::new();
    let open = TypeKind::Generic(
        "T".to_string(),
        None,
        miri::ast::types::TypeDeclarationKind::None,
    );
    assert!(mentions_open_parameter(&ty(list_of(open)), &defs));
    assert!(mentions_open_parameter(
        &ty(TypeKind::Custom("U".to_string(), None)),
        &defs
    ));
    assert!(!mentions_open_parameter(
        &ty(list_of(TypeKind::String)),
        &defs
    ));
    assert!(!mentions_open_parameter(&ty(TypeKind::RawPtr), &defs));
}

/// A type argument with no name to compile a body at is refused with the
/// instantiation's code, naming the instance and the argument.
#[test]
fn an_unnameable_type_argument_is_refused_naming_the_instance() {
    let meta = ty(TypeKind::Meta(Box::new(ty(TypeKind::Int))));
    let error = unnameable_type_argument("W", &[meta.clone()], &meta, Span::new(0, 0));
    let properties = error.kind.properties();
    assert_eq!(
        properties.code,
        miri::diagnostics::DiagnosticCode::MirInvalidInstantiationArgument
    );
    assert_eq!(
        properties.message.as_deref(),
        Some("instantiating `W<int>`: `int` has no name the compiler can compile a body at")
    );
    assert_eq!(properties.help.as_deref(), Some(TYPE_ARGUMENT_HELP));
}

/// Steps of a growth chain nested past the levels a step is shown to are
/// spelled out until they differ, never shown as one elided spelling twice.
#[test]
fn a_growth_note_never_shows_two_identical_steps() {
    let wrapped = |levels: usize| {
        vec![ty((0..levels).fold(TypeKind::Int, |inner, _| {
            generic("Wrap", vec![inner])
        }))]
    };
    let (five, six, seven) = (wrapped(5), wrapped(6), wrapped(7));
    let growth = Growth {
        chain: vec![five.as_slice(), six.as_slice()],
        method: Some("depth"),
        through_trait: false,
    };
    let error = polymorphic_recursion(
        "Impl",
        &seven,
        &ExceededLimit::TypeDepth(8),
        &growth,
        Span::new(0, 0),
    );
    let report = error.report("");
    let note = report
        .lines()
        .find(|line| line.contains("note:"))
        .unwrap_or_else(|| panic!("no note in:\n{report}"));
    let steps: Vec<&str> = note
        .split(": ")
        .last()
        .unwrap_or_default()
        .split(" → ")
        .collect();
    assert_eq!(steps.len(), 3, "{note}");
    assert!(steps[0] != steps[1] && steps[1] != steps[2], "{note}");
}
