// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{make_int_const, make_string_const};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::{AggregateKind, Rvalue};

fn assert_display(rvalue: &Rvalue, expected: &str) {
    assert_eq!(format!("{}", rvalue), expected);
}

#[test]
fn test_aggregate_tuple_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Tuple,
        vec![make_int_const(1), make_int_const(2)],
    );
    assert_display(&rvalue, "(const Integer(I32(1)), const Integer(I32(2)))");
}

#[test]
fn test_aggregate_array_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Array,
        vec![make_int_const(1), make_int_const(2), make_int_const(3)],
    );
    assert_display(
        &rvalue,
        "[const Integer(I32(1)), const Integer(I32(2)), const Integer(I32(3))]",
    );
}

#[test]
fn test_aggregate_list_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::List,
        vec![make_int_const(1), make_int_const(2)],
    );
    assert_display(&rvalue, "[const Integer(I32(1)), const Integer(I32(2))]");
}

#[test]
fn test_aggregate_set_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Set,
        vec![make_int_const(1), make_int_const(2)],
    );
    assert_display(&rvalue, "{const Integer(I32(1)), const Integer(I32(2))}");
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
    assert_display(
        &rvalue,
        "{const String(\"a\"): const Integer(I32(1)), const String(\"b\"): const Integer(I32(2))}",
    );
}

#[test]
fn test_aggregate_struct_display() {
    let ty = Type::new(TypeKind::Custom("Point".to_string(), None), Span::default());
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Struct(ty),
        vec![make_int_const(10), make_int_const(20)],
    );
    assert_display(
        &rvalue,
        "Point { const Integer(I32(10)), const Integer(I32(20)) }",
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
