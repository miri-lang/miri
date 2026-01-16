// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{
    make_int_const, make_string_const, mir_rvalue_display_contains_test,
    mir_rvalue_display_ends_with_test, mir_rvalue_display_starts_with_test,
    mir_rvalue_equality_test,
};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::{AggregateKind, Rvalue};

#[test]
fn test_aggregate_tuple_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Tuple,
        vec![make_int_const(1), make_int_const(2)],
    );
    mir_rvalue_display_starts_with_test(&rvalue, "(");
    mir_rvalue_display_ends_with_test(&rvalue, ")");
}

#[test]
fn test_aggregate_array_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Array,
        vec![make_int_const(1), make_int_const(2), make_int_const(3)],
    );
    mir_rvalue_display_starts_with_test(&rvalue, "[");
    mir_rvalue_display_ends_with_test(&rvalue, "]");
}

#[test]
fn test_aggregate_list_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::List,
        vec![make_int_const(1), make_int_const(2)],
    );
    mir_rvalue_display_starts_with_test(&rvalue, "[");
    mir_rvalue_display_ends_with_test(&rvalue, "]");
}

#[test]
fn test_aggregate_set_display() {
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Set,
        vec![make_int_const(1), make_int_const(2)],
    );
    mir_rvalue_display_starts_with_test(&rvalue, "{");
    mir_rvalue_display_ends_with_test(&rvalue, "}");
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
    mir_rvalue_display_starts_with_test(&rvalue, "{");
    mir_rvalue_display_ends_with_test(&rvalue, "}");
    mir_rvalue_display_contains_test(&rvalue, ":");
}

#[test]
fn test_aggregate_struct_display() {
    let ty = Type::new(TypeKind::Custom("Point".to_string(), None), Span::default());
    let rvalue = Rvalue::Aggregate(
        AggregateKind::Struct(ty),
        vec![make_int_const(10), make_int_const(20)],
    );
    mir_rvalue_display_contains_test(&rvalue, "Point");
    mir_rvalue_display_contains_test(&rvalue, "{");
    mir_rvalue_display_contains_test(&rvalue, "}");
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
    mir_rvalue_equality_test(&a, &b);
}

#[test]
fn test_aggregate_cloning() {
    let original = Rvalue::Aggregate(
        AggregateKind::Array,
        vec![make_int_const(1), make_int_const(2)],
    );
    let cloned = original.clone();
    mir_rvalue_equality_test(&original, &cloned);
}
