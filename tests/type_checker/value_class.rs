// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The value-class authority, judged over types the type checker builds from
//! real source: each case declares its types and spells the type asked about
//! as a parameter of `probe`.

use crate::type_checker::utils::type_checker_result;
use miri::ast::expression::ExpressionKind;
use miri::ast::factory::{make_type, type_expr_non_null};
use miri::ast::types::{Type, TypeKind, ATOMIC_TYPE_NAME, VEC3_TYPE_NAME};
use miri::ast::StatementKind;
use miri::type_checker::context::TypeDefinition;
use miri::type_checker::value_class::{
    value_class, FieldRepr, LayoutScope, ScalarKind, ValueClass, ValueField, ValueLayout, WordRole,
};
use std::collections::{HashMap, HashSet};

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

    fn class_of(&self, parameter: &str) -> ValueClass {
        self.class_in(parameter, &HashSet::new())
    }

    fn class_in(&self, parameter: &str, open: &HashSet<String>) -> ValueClass {
        let ty = &self.parameters[parameter];
        value_class(&ty.kind, LayoutScope::new(&self.definitions, open))
    }

    fn layout_of(&self, parameter: &str) -> ValueLayout {
        match self.class_of(parameter) {
            ValueClass::Bitwise(layout) => layout,
            other => panic!("`{parameter}` should be a bitwise value, got {other:?}"),
        }
    }
}

fn declared_type(node: &ExpressionKind) -> Type {
    match node {
        ExpressionKind::Type(ty, false) => (**ty).clone(),
        ExpressionKind::Type(ty, true) => Type::new(TypeKind::Option(ty.clone()), ty.span),
        other => panic!("parameter type is not a type: {other:?}"),
    }
}

fn scalar_at(offset: u32, kind: ScalarKind) -> ValueField {
    ValueField {
        offset,
        repr: FieldRepr::Scalar(kind),
    }
}

const INT: ScalarKind = ScalarKind::Signed(8);
const FLOAT: ScalarKind = ScalarKind::Float(8);

const DECLARATIONS: &str = "
struct P
    x int
    y float

struct Pair<A, B>
    first A
    second B

struct Point3
    x float
    y float
    z float

struct Sphere
    center Point3
    radius float

struct Op
    run fn(x int) int

type Meters is float

type MaybeInt is int?

struct Mixed
    small i8
    whole int
    single f32

class Node
    var value int

struct Nine
    a int
    b int
    c int
    d int
    e int
    f int
    g int
    h int
    i int

struct NineF
    a float
    b float
    c float
    d float
    e float
    f float
    g float
    h float
    i float

struct Both
    ints Nine
    floats NineF

struct V
    x int

struct Holder
    v V
";

fn probe() -> Probe {
    Probe::of(&format!(
        "{DECLARATIONS}
fn probe(p P, pair Pair<int, float>, named Pair<String, int>, sphere Sphere, ints (int, int), mixed_tuple (int, String), op Op, meters Meters, mixed Mixed, maybe int?, node Node?, nine Nine, both Both, v V, holder Holder, twice MaybeInt?, maybe_p P?)
    return

fn main()
    probe(P(1, 2.0), Pair(1, 2.0), Pair(\"a\", 1), Sphere(Point3(0.0, 0.0, 0.0), 1.0), (1, 2), (1, \"a\"), Op(fn(x int) int: x), 1.0, Mixed(1, 2, 3.0), None, None, Nine(1, 2, 3, 4, 5, 6, 7, 8, 9), Both(Nine(1, 2, 3, 4, 5, 6, 7, 8, 9), NineF(1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0)), V(1), Holder(V(1)), None, None)
"
    ))
}

#[test]
fn value_class_scalar_struct_is_bitwise_16() {
    let layout = probe().layout_of("p");
    assert_eq!(
        layout,
        ValueLayout {
            size: 16,
            align: 8,
            fields: vec![scalar_at(0, INT), scalar_at(8, FLOAT)],
        }
    );
}

#[test]
fn value_class_generic_struct_is_decided_at_its_arguments() {
    let layout = probe().layout_of("pair");
    assert_eq!(layout.size, 16);
    assert_eq!(layout.fields, vec![scalar_at(0, INT), scalar_at(8, FLOAT)]);
}

#[test]
fn value_class_generic_struct_with_managed_argument_is_managed() {
    assert_eq!(probe().class_of("named"), ValueClass::Managed);
}

#[test]
fn value_class_generic_struct_over_open_parameters_is_managed() {
    let probe = Probe::of(&format!(
        "{DECLARATIONS}
fn probe<T, U>(open Pair<T, U>, bare T)
    return
"
    ));
    let open: HashSet<String> = ["T", "U"].map(String::from).into();
    assert_eq!(probe.class_in("open", &open), ValueClass::Managed);
    assert_eq!(
        probe.class_in("bare", &open),
        ValueClass::Word(WordRole::Parameter)
    );
}

#[test]
fn value_class_nested_struct_is_inline_at_its_offset() {
    let layout = probe().layout_of("sphere");
    assert_eq!(layout.size, 32);
    let center = ValueLayout {
        size: 24,
        align: 8,
        fields: vec![
            scalar_at(0, FLOAT),
            scalar_at(8, FLOAT),
            scalar_at(16, FLOAT),
        ],
    };
    assert_eq!(
        layout.fields,
        vec![
            ValueField {
                offset: 0,
                repr: FieldRepr::Value(center),
            },
            scalar_at(24, FLOAT),
        ]
    );
}

#[test]
fn value_class_scalar_tuple_is_bitwise_16() {
    let layout = probe().layout_of("ints");
    assert_eq!(layout.size, 16);
    assert_eq!(layout.fields, vec![scalar_at(0, INT), scalar_at(8, INT)]);
}

#[test]
fn value_class_tuple_with_string_is_managed() {
    assert_eq!(probe().class_of("mixed_tuple"), ValueClass::Managed);
}

#[test]
fn value_class_struct_with_function_field_is_managed() {
    assert_eq!(probe().class_of("op"), ValueClass::Managed);
}

#[test]
fn value_class_scalar_alias_is_scalar() {
    assert_eq!(probe().class_of("meters"), ValueClass::Scalar(FLOAT));
}

#[test]
fn value_class_fields_are_naturally_aligned() {
    let layout = probe().layout_of("mixed");
    assert_eq!(
        layout,
        ValueLayout {
            size: 24,
            align: 8,
            fields: vec![
                scalar_at(0, ScalarKind::Signed(1)),
                scalar_at(8, INT),
                scalar_at(16, ScalarKind::Float(4)),
            ],
        }
    );
}

#[test]
fn value_class_optional_scalar_is_tag_then_payload() {
    let layout = probe().layout_of("maybe");
    assert_eq!(
        layout,
        ValueLayout {
            size: 16,
            align: 8,
            fields: vec![
                ValueField {
                    offset: 0,
                    repr: FieldRepr::OptionTag,
                },
                scalar_at(8, INT),
            ],
        }
    );
}

#[test]
fn value_class_nested_optional_scalar_carries_two_tags() {
    let layout = probe().layout_of("twice");
    assert_eq!(layout.size, 24);
    let inner = probe().layout_of("maybe");
    assert_eq!(
        layout.fields,
        vec![
            ValueField {
                offset: 0,
                repr: FieldRepr::OptionTag,
            },
            ValueField {
                offset: 8,
                repr: FieldRepr::Value(inner),
            },
        ]
    );
}

#[test]
fn value_class_optional_struct_is_bitwise() {
    let layout = probe().layout_of("maybe_p");
    assert_eq!(layout.size, 24);
    assert_eq!(layout.fields[1].offset, 8);
}

#[test]
fn value_class_optional_class_is_a_nullable_pointer_word() {
    assert_eq!(
        probe().class_of("node"),
        ValueClass::Word(WordRole::NullablePointer)
    );
}

#[test]
fn value_class_struct_over_the_size_limit_is_managed() {
    let probe = probe();
    assert_eq!(probe.layout_of("nine").size, 72);
    assert_eq!(probe.class_of("both"), ValueClass::Managed);
}

#[test]
fn value_class_bare_name_open_in_the_body_is_the_parameter() {
    let probe = probe();
    let open: HashSet<String> = ["V"].map(String::from).into();
    assert_eq!(
        probe.class_in("v", &open),
        ValueClass::Word(WordRole::Parameter)
    );
    assert_eq!(probe.layout_of("v").size, 8);
}

#[test]
fn value_class_field_named_like_an_open_parameter_is_the_program_type() {
    let probe = probe();
    let open: HashSet<String> = ["V"].map(String::from).into();
    match probe.class_in("holder", &open) {
        ValueClass::Bitwise(layout) => assert_eq!(layout.size, 8),
        other => panic!("`Holder` should be a bitwise value, got {other:?}"),
    }
}

#[test]
fn value_class_unit_tuple_is_an_empty_value() {
    let definitions = HashMap::new();
    let class = value_class(
        &TypeKind::Tuple(Vec::new()),
        LayoutScope::program(&definitions),
    );
    assert_eq!(
        class,
        ValueClass::Bitwise(ValueLayout {
            size: 0,
            align: 1,
            fields: Vec::new(),
        })
    );
}

#[test]
fn value_class_self_referential_struct_terminates_as_managed() {
    let probe = Probe::of(
        "
struct Link
    value int
    next Link?

fn probe(link Link)
    return
",
    );
    assert_eq!(probe.class_of("link"), ValueClass::Managed);
}

fn written_with_component(name: &str, component: TypeKind) -> TypeKind {
    TypeKind::Custom(
        name.to_string(),
        Some(vec![type_expr_non_null(make_type(component))]),
    )
}

#[test]
fn value_class_inline_vector_keeps_its_managed_representation() {
    let definitions = HashMap::new();
    let vector = written_with_component(VEC3_TYPE_NAME, TypeKind::F32);
    assert_eq!(
        value_class(&vector, LayoutScope::program(&definitions)),
        ValueClass::Managed
    );
}

#[test]
fn value_class_atomic_is_held_as_its_component() {
    let definitions = HashMap::new();
    let atomic = written_with_component(ATOMIC_TYPE_NAME, TypeKind::U32);
    assert_eq!(
        value_class(&atomic, LayoutScope::program(&definitions)),
        ValueClass::Scalar(ScalarKind::Unsigned(4))
    );
}

#[test]
fn value_class_program_struct_named_like_a_vector_is_an_ordinary_struct() {
    let probe = Probe::of(
        "
struct Vec3
    x float
    y float
    z float

fn probe(v Vec3)
    return
",
    );
    assert_eq!(probe.layout_of("v").size, 24);
}
