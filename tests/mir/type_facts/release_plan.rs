// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What freeing a value does once its count reaches zero, and which captures
//! a closure's destructor releases.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::factory::type_expr_non_null;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::option_layout::LayoutScope;
use miri::mir::type_facts::{
    release_plan, released_captures, CountedSlot, ReleasePlan, TypeDefinition, UnresolvedArgument,
};
use miri::type_checker::context::AliasDefinition;
use std::collections::{HashMap, HashSet};

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn arg(kind: TypeKind) -> Box<Expression> {
    Box::new(type_expr_non_null(ty(kind)))
}

fn plan(kind: &TypeKind) -> Result<ReleasePlan, UnresolvedArgument> {
    plan_in(kind, &HashMap::new())
}

fn plan_in(
    kind: &TypeKind,
    definitions: &HashMap<String, TypeDefinition>,
) -> Result<ReleasePlan, UnresolvedArgument> {
    let open = HashSet::new();
    release_plan(kind, LayoutScope::new(definitions, &open))
}

#[test]
fn a_map_of_strings_releases_its_values_and_a_map_of_ints_does_not() {
    let strings = TypeKind::Map(arg(TypeKind::Int), arg(TypeKind::String));
    assert_eq!(
        plan(&strings),
        Ok(ReleasePlan::Map {
            counted_value: Some(ty(TypeKind::String))
        })
    );
    let ints = TypeKind::Map(arg(TypeKind::String), arg(TypeKind::Int));
    assert_eq!(
        plan(&ints),
        Ok(ReleasePlan::Map {
            counted_value: None
        })
    );
}

#[test]
fn a_list_releases_its_elements_only_when_they_are_counted() {
    let strings = TypeKind::List(arg(TypeKind::String));
    assert_eq!(
        plan(&strings),
        Ok(ReleasePlan::List {
            counted_element: Some(ty(TypeKind::String))
        })
    );
    let floats = TypeKind::List(arg(TypeKind::Float));
    assert_eq!(
        plan(&floats),
        Ok(ReleasePlan::List {
            counted_element: None
        })
    );
}

#[test]
fn a_tuple_releases_exactly_its_counted_slots_by_position() {
    let tuple = TypeKind::Tuple(vec![
        *arg(TypeKind::Int),
        *arg(TypeKind::String),
        *arg(TypeKind::Boolean),
        *arg(TypeKind::List(arg(TypeKind::Int))),
    ]);
    assert_eq!(
        plan(&tuple),
        Ok(ReleasePlan::Tuple {
            counted: vec![
                CountedSlot {
                    index: 1,
                    ty: ty(TypeKind::String)
                },
                CountedSlot {
                    index: 3,
                    ty: ty(TypeKind::List(arg(TypeKind::Int)))
                },
            ]
        })
    );
}

#[test]
fn a_collection_whose_type_argument_names_no_type_is_refused() {
    let unresolved = Expression::new(
        0,
        ExpressionKind::Identifier("T".to_string(), None),
        Span::default(),
    );
    let list = TypeKind::List(Box::new(unresolved));
    let Err(UnresolvedArgument(message)) = plan(&list) else {
        panic!("a list released at an unresolved argument must be refused");
    };
    assert!(message.contains("unresolved type argument"), "{message}");
}

#[test]
fn an_alias_is_released_as_the_type_it_stands_for() {
    let definitions = HashMap::from([(
        "Names".to_string(),
        TypeDefinition::Alias(AliasDefinition {
            template: ty(TypeKind::List(arg(TypeKind::String))),
            generics: None,
        }),
    )]);
    assert_eq!(
        plan_in(&TypeKind::Custom("Names".to_string(), None), &definitions),
        Ok(ReleasePlan::List {
            counted_element: Some(ty(TypeKind::String))
        })
    );
}

#[test]
fn a_closure_destructor_releases_only_its_counted_captures() {
    let definitions = HashMap::new();
    let open = HashSet::new();
    let captures = [
        TypeKind::Int,
        TypeKind::String,
        TypeKind::Float,
        TypeKind::List(arg(TypeKind::Int)),
    ];
    assert_eq!(
        released_captures(captures.iter(), LayoutScope::new(&definitions, &open)),
        vec![1, 3]
    );
}
