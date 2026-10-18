// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The element-layout authority, judged over types the type checker builds
//! from real source: each case spells the element type asked about as a
//! parameter of `probe`, so the type table the layout is decided against is
//! the program's own.

use crate::type_checker::utils::type_checker_result;
use miri::ast::expression::ExpressionKind;
use miri::ast::types::{Type, TypeKind};
use miri::ast::StatementKind;
use miri::type_checker::context::TypeDefinition;
use miri::type_checker::element_layout::{element_layout, ElementLayout};
use miri::type_checker::value_class::LayoutScope;
use std::collections::HashMap;

const SOURCE: &str = "
use system.gpu.vector

struct Pt
    x int
    y int

class Node
    var value int

fn probe(whole int, small i8, single f32, wide i128, text String, node Node, pt Pt, v Vec3<f32>, pair (int, int), maybe int?, maybe_node Node?)
    return
";

/// The program's type table and the declared type of every `probe` parameter.
struct Probe {
    definitions: HashMap<String, TypeDefinition>,
    parameters: HashMap<String, Type>,
}

impl Probe {
    fn of(source: &str) -> Self {
        let result = type_checker_result(source);
        let parameters = result
            .ast
            .body
            .iter()
            .find_map(|statement| match &statement.node {
                StatementKind::FunctionDeclaration(function) if function.name == "probe" => Some(
                    function
                        .params
                        .iter()
                        .map(|param| (param.name.clone(), declared_type(&param.typ.node))),
                ),
                _ => None,
            })
            .expect("source declares `probe`")
            .collect();
        Self {
            definitions: result.type_checker.type_definitions().clone(),
            parameters,
        }
    }

    fn layout_of(&self, parameter: &str) -> ElementLayout {
        element_layout(
            &self.parameters[parameter].kind,
            LayoutScope::program(&self.definitions),
        )
    }
}

fn declared_type(node: &ExpressionKind) -> Type {
    match node {
        ExpressionKind::Type(ty, false) => (**ty).clone(),
        ExpressionKind::Type(ty, true) => Type::new(TypeKind::Option(ty.clone()), ty.span),
        other => panic!("parameter type is not a type: {other:?}"),
    }
}

fn held_by_value(bytes: i64) -> ElementLayout {
    ElementLayout {
        payload: bytes,
        stride: bytes,
        is_address: false,
    }
}

#[test]
fn scalar_elements_occupy_their_own_width() {
    let probe = Probe::of(SOURCE);
    assert_eq!(probe.layout_of("whole"), held_by_value(8));
    assert_eq!(probe.layout_of("small"), held_by_value(1));
    assert_eq!(probe.layout_of("single"), held_by_value(4));
    assert_eq!(probe.layout_of("wide"), held_by_value(16));
}

#[test]
fn half_float_element_is_held_in_a_value_word() {
    let definitions = HashMap::new();
    assert_eq!(
        element_layout(&TypeKind::F16, LayoutScope::program(&definitions)),
        held_by_value(8)
    );
}

#[test]
fn referenced_and_optional_elements_are_held_in_a_value_word() {
    let probe = Probe::of(SOURCE);
    for parameter in ["text", "node", "maybe", "maybe_node"] {
        assert_eq!(
            probe.layout_of(parameter),
            held_by_value(8),
            "`{parameter}` should be held in a value word"
        );
    }
}

#[test]
fn struct_and_tuple_elements_of_scalars_are_held_inline_as_their_bytes() {
    let probe = Probe::of(SOURCE);
    for parameter in ["pt", "pair"] {
        assert_eq!(
            probe.layout_of(parameter),
            ElementLayout {
                payload: 16,
                stride: 16,
                is_address: false,
            },
            "`{parameter}` should be held inline as its sixteen bytes"
        );
    }
}

#[test]
fn vector_element_is_laid_out_inline_at_its_std430_stride() {
    let probe = Probe::of(SOURCE);
    assert_eq!(
        probe.layout_of("v"),
        ElementLayout {
            payload: 12,
            stride: 16,
            is_address: true,
        }
    );
}
