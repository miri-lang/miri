// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The one signature every Miri function is declared, defined and called
//! under. A bitwise value held as its bytes is passed by address; it comes
//! back in registers, one per scalar member, when its members fit two
//! integer and two float registers, and through a leading return-slot
//! pointer otherwise. Every other value travels as one word of its own type.

use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_codegen::isa::CallConv;
use miri::ast::factory::type_expr_non_null;
use miri::ast::types::{Type, TypeKind};
use miri::ast::MemberVisibility;
use miri::codegen::cranelift::value_abi::{body_signature, miri_signature, Passing, Returning};
use miri::error::syntax::Span;
use miri::mir::rc::RcClassifier;
use miri::mir::{Body, ExecutionModel, LocalDecl};
use miri::type_checker::context::{StructDefinition, TypeDefinition};
use std::collections::HashMap;
use std::rc::Rc;

const PTR: types::Type = types::I64;
const CALL_CONV: CallConv = CallConv::SystemV;

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn point() -> TypeKind {
    TypeKind::Custom("P".to_string(), None)
}

/// `struct V3 { x float, y float, z float }`: three floats, one more than
/// the float registers a result comes back in.
fn vec3() -> TypeKind {
    TypeKind::Custom("V3".to_string(), None)
}

fn tuple_of(kinds: &[TypeKind]) -> TypeKind {
    TypeKind::Tuple(
        kinds
            .iter()
            .map(|kind| type_expr_non_null(ty(kind.clone())))
            .collect(),
    )
}

fn optional(kind: TypeKind) -> TypeKind {
    TypeKind::Option(Box::new(ty(kind)))
}

/// A struct of public `fields`, with no managed interior.
fn struct_of(fields: &[(&str, TypeKind)]) -> TypeDefinition {
    let fields = fields
        .iter()
        .map(|(name, kind)| (name.to_string(), ty(kind.clone()), MemberVisibility::Public))
        .collect();
    TypeDefinition::Struct(StructDefinition {
        fields,
        generics: None,
        module: String::new(),
    })
}

/// `struct Q { a int, b float, c int, d float }`: two integer and two float
/// members, exactly the registers a result comes back in.
fn quad() -> TypeKind {
    TypeKind::Custom("Q".to_string(), None)
}

/// `struct P { x int, y float }`, `struct V3 { x float, y float, z float }`
/// and `struct Q { a int, b float, c int, d float }`.
fn definitions() -> Rc<HashMap<String, TypeDefinition>> {
    let point = struct_of(&[("x", TypeKind::Int), ("y", TypeKind::Float)]);
    let vec3 = struct_of(&[
        ("x", TypeKind::Float),
        ("y", TypeKind::Float),
        ("z", TypeKind::Float),
    ]);
    let quad = struct_of(&[
        ("a", TypeKind::Int),
        ("b", TypeKind::Float),
        ("c", TypeKind::Int),
        ("d", TypeKind::Float),
    ]);
    Rc::new(HashMap::from([
        ("P".to_string(), point),
        ("V3".to_string(), vec3),
        ("Q".to_string(), quad),
    ]))
}

/// A body returning `result` and taking `params`.
fn body_of(result: TypeKind, params: &[TypeKind]) -> Body {
    let mut body = Body::new(params.len(), Span::default(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ty(result), Span::default()));
    for kind in params {
        body.new_local(LocalDecl::new(ty(kind.clone()), Span::default()));
    }
    body.rc_classifier = RcClassifier::new(definitions());
    body
}

fn signature_of(params: &[types::Type], returns: &[types::Type]) -> Signature {
    let mut signature = Signature::new(CALL_CONV);
    signature.params = params.iter().map(|&t| AbiParam::new(t)).collect();
    signature.returns = returns.iter().map(|&t| AbiParam::new(t)).collect();
    signature
}

#[test]
fn a_result_through_a_slot_becomes_a_leading_pointer_parameter() {
    let signature = miri_signature(
        CALL_CONV,
        PTR,
        &[Passing::Word(types::F64), Passing::ByAddress],
        Returning::ThroughSlot,
    );
    assert_eq!(signature, signature_of(&[PTR, types::F64, PTR], &[]));
}

#[test]
fn word_parameters_and_a_word_result_keep_their_own_types() {
    let signature = miri_signature(
        CALL_CONV,
        PTR,
        &[Passing::Word(types::I8), Passing::Word(types::F32)],
        Returning::Word(types::I32),
    );
    assert_eq!(
        signature,
        signature_of(&[types::I8, types::F32], &[types::I32])
    );
}

#[test]
fn a_function_returning_nothing_has_no_result_and_no_slot() {
    let signature = miri_signature(CALL_CONV, PTR, &[], Returning::Nothing);
    assert_eq!(signature, signature_of(&[], &[]));
}

#[test]
fn a_struct_of_scalars_is_taken_by_address_and_returned_in_registers() {
    let body = body_of(point(), &[point(), TypeKind::Float]);
    let signature = body_signature(&body, CALL_CONV, PTR);
    assert_eq!(
        signature,
        signature_of(&[PTR, types::F64], &[types::I64, types::F64])
    );
}

#[test]
fn an_optional_int_comes_back_as_its_tag_and_its_payload_with_no_slot() {
    let body = body_of(optional(TypeKind::Int), &[]);
    let signature = body_signature(&body, CALL_CONV, PTR);
    assert_eq!(signature, signature_of(&[], &[types::I8, types::I64]));
}

#[test]
fn a_tuple_of_two_ints_comes_back_as_two_ints() {
    let pair = tuple_of(&[TypeKind::Int, TypeKind::Int]);
    let body = body_of(pair.clone(), &[pair]);
    let signature = body_signature(&body, CALL_CONV, PTR);
    assert_eq!(signature, signature_of(&[PTR], &[types::I64, types::I64]));
}

#[test]
fn an_optional_float_comes_back_as_its_tag_and_a_float() {
    let body = body_of(optional(TypeKind::Float), &[]);
    let signature = body_signature(&body, CALL_CONV, PTR);
    assert_eq!(signature, signature_of(&[], &[types::I8, types::F64]));
}

/// Each member comes back at its own type: a narrow integer and a `bool` as
/// that width, an `f32` as a single-precision float.
#[test]
fn narrow_members_come_back_at_their_own_widths() {
    let cases = [
        (
            tuple_of(&[TypeKind::I8, TypeKind::F32]),
            vec![types::I8, types::F32],
        ),
        (
            tuple_of(&[TypeKind::I16, TypeKind::Boolean]),
            vec![types::I16, types::I8],
        ),
        (optional(TypeKind::Boolean), vec![types::I8, types::I8]),
        (optional(TypeKind::F32), vec![types::I8, types::F32]),
        (
            tuple_of(&[TypeKind::F32, TypeKind::Int, TypeKind::F32, TypeKind::I16]),
            vec![types::F32, types::I64, types::F32, types::I16],
        ),
    ];
    for (result, returns) in cases {
        let body = body_of(result.clone(), &[]);
        let signature = body_signature(&body, CALL_CONV, PTR);
        assert_eq!(
            signature,
            signature_of(&[], &returns),
            "{result:?} comes back member by member"
        );
    }
}

/// A result using every register the budget allows still comes back in
/// registers: two integers and two floats, or two floats alone.
#[test]
fn a_result_filling_the_registers_exactly_comes_back_in_them() {
    let cases = [
        (quad(), vec![types::I64, types::F64, types::I64, types::F64]),
        (
            tuple_of(&[TypeKind::Float, TypeKind::Float]),
            vec![types::F64, types::F64],
        ),
    ];
    for (result, returns) in cases {
        let body = body_of(result.clone(), &[]);
        let signature = body_signature(&body, CALL_CONV, PTR);
        assert_eq!(
            signature,
            signature_of(&[], &returns),
            "{result:?} fills the registers"
        );
    }
}

/// Two integer and two float registers carry a result back: a third float,
/// a third integer, or a scalar wider than a register sends it through the
/// caller's slot.
#[test]
fn a_result_wider_than_the_registers_still_comes_back_through_a_slot() {
    for result in [
        vec3(),
        tuple_of(&[TypeKind::Int, TypeKind::Int, TypeKind::Int]),
        tuple_of(&[TypeKind::Boolean, TypeKind::Boolean, TypeKind::I8]),
        optional(tuple_of(&[TypeKind::Int, TypeKind::Int])),
        optional(TypeKind::I128),
    ] {
        let body = body_of(result.clone(), &[TypeKind::Int]);
        let signature = body_signature(&body, CALL_CONV, PTR);
        assert_eq!(
            signature,
            signature_of(&[PTR, types::I64], &[]),
            "{result:?} comes back through the caller's slot"
        );
    }
}

#[test]
fn a_body_over_counted_values_takes_and_returns_their_pointers() {
    let optional = TypeKind::Option(Box::new(ty(TypeKind::String)));
    let body = body_of(optional.clone(), &[optional, TypeKind::Float]);
    let signature = body_signature(&body, CALL_CONV, PTR);
    assert_eq!(signature, signature_of(&[PTR, types::F64], &[PTR]));
}

#[test]
fn a_body_over_scalars_takes_and_returns_them_as_words() {
    let body = body_of(TypeKind::I32, &[TypeKind::Boolean]);
    let signature = body_signature(&body, CALL_CONV, PTR);
    assert_eq!(signature, signature_of(&[types::I8], &[types::I32]));
}

#[test]
fn a_managed_struct_is_never_held_as_bytes() {
    let body = body_of(TypeKind::String, &[TypeKind::String]);
    let signature = body_signature(&body, CALL_CONV, PTR);
    assert_eq!(signature, signature_of(&[PTR], &[PTR]));
}

/// An optional of a scalar or of a value held as its bytes is itself held as
/// its bytes: taken by address and returned in registers, its tag first,
/// however the optional is spelled.
#[test]
fn a_body_over_optionals_held_as_bytes_takes_them_by_address_and_returns_their_members() {
    let written_option = |payload: TypeKind| {
        TypeKind::Custom(
            "Option".to_string(),
            Some(vec![miri::ast::expression::Expression::new(
                0,
                miri::ast::expression::ExpressionKind::Type(Box::new(ty(payload)), false),
                Span::default(),
            )]),
        )
    };
    let of_i32 = [types::I8, types::I32];
    let of_point = [types::I8, types::I64, types::F64];
    for (optional, returns) in [
        (TypeKind::Option(Box::new(ty(TypeKind::I32))), &of_i32[..]),
        (TypeKind::Option(Box::new(ty(point()))), &of_point[..]),
        (written_option(TypeKind::I32), &of_i32[..]),
        (written_option(point()), &of_point[..]),
    ] {
        let body = body_of(optional.clone(), &[optional.clone()]);
        let signature = body_signature(&body, CALL_CONV, PTR);
        assert_eq!(
            signature,
            signature_of(&[PTR], returns),
            "{optional:?} is passed by address and returned in registers"
        );
    }
}
