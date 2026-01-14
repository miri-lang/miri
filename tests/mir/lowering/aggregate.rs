// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{make_int_const, make_string_const};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::{AggregateKind, Rvalue};

#[test]
fn test_aggregate_tuple_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Tuple,
        vec![make_int_const(1), make_int_const(2)],
    );
    let display = format!("{}", rvalue);
    assert!(
        display.starts_with("("),
        "Tuple should start with '(': {}",
        display
    );
    assert!(
        display.ends_with(")"),
        "Tuple should end with ')': {}",
        display
    );
}

#[test]
fn test_aggregate_array_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Array,
        vec![make_int_const(1), make_int_const(2), make_int_const(3)],
    );
    let display = format!("{}", rvalue);
    assert!(
        display.starts_with("["),
        "Array should start with '[': {}",
        display
    );
    assert!(
        display.ends_with("]"),
        "Array should end with ']': {}",
        display
    );
}

#[test]
fn test_aggregate_list_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::List,
        vec![make_int_const(1), make_int_const(2)],
    );
    let display = format!("{}", rvalue);
    assert!(
        display.starts_with("["),
        "List should start with '[': {}",
        display
    );
    assert!(
        display.ends_with("]"),
        "List should end with ']': {}",
        display
    );
}

#[test]
fn test_aggregate_set_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Set,
        vec![make_int_const(1), make_int_const(2)],
    );
    let display = format!("{}", rvalue);
    assert!(
        display.starts_with("{"),
        "Set should start with '{{': {}",
        display
    );
    assert!(
        display.ends_with("}"),
        "Set should end with '}}': {}",
        display
    );
}

#[test]
fn test_aggregate_map_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Map,
        vec![
            make_string_const("a"),
            make_int_const(1),
            make_string_const("b"),
            make_int_const(2),
        ],
    );
    let display = format!("{}", rvalue);
    assert!(
        display.starts_with("{"),
        "Map should start with '{{': {}",
        display
    );
    assert!(
        display.ends_with("}"),
        "Map should end with '}}': {}",
        display
    );
    assert!(display.contains(":"), "Map should contain ':': {}", display);
}

#[test]
fn test_aggregate_struct_display() {
    let ty = Type::new(TypeKind::Custom("Point".to_string(), None), Span::default());
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Struct(ty),
        vec![make_int_const(10), make_int_const(20)],
    );
    let display = format!("{}", rvalue);
    assert!(
        display.contains("Point"),
        "Struct should contain type name: {}",
        display
    );
    assert!(
        display.contains("{"),
        "Struct should contain '{{': {}",
        display
    );
    assert!(
        display.contains("}"),
        "Struct should contain '}}': {}",
        display
    );
}

#[test]
fn test_aggregate_equality() {
    let a = Rvalue::Aggregate(
        AggregateKind::Tuple,
        vec![make_int_const(1), make_int_const(2)],
    );
    let b = Rvalue::Aggregate(
        AggregateKind::Tuple,
        vec![make_int_const(1), make_int_const(2)],
    );
    assert_eq!(a, b);
}

#[test]
fn test_aggregate_cloning() {
    let original = Rvalue::Aggregate(
        AggregateKind::Array,
        vec![make_int_const(1), make_int_const(2)],
    );
    let cloned = original.clone();
    assert_eq!(original, cloned);
}
