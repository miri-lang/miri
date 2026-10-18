// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The one signature every Miri function is declared, defined and called
//! under. A bitwise value held as its bytes travels by address — a parameter
//! as a pointer, a result through a leading return-slot pointer — and every
//! other value as one word of its own type.

use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_codegen::isa::CallConv;
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

/// `struct P { x int, y float }`: sixteen bytes, no managed interior.
fn definitions() -> Rc<HashMap<String, TypeDefinition>> {
    let fields = [("x", TypeKind::Int), ("y", TypeKind::Float)]
        .into_iter()
        .map(|(name, kind)| (name.to_string(), ty(kind), MemberVisibility::Public))
        .collect();
    let point = StructDefinition {
        fields,
        generics: None,
        module: String::new(),
    };
    Rc::new(HashMap::from([(
        "P".to_string(),
        TypeDefinition::Struct(point),
    )]))
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
fn a_program_takes_and_returns_a_struct_of_scalars_by_address() {
    let body = body_of(point(), &[point(), TypeKind::Float]);
    let signature = body_signature(&body, CALL_CONV, PTR);
    assert_eq!(signature, signature_of(&[PTR, PTR, types::F64], &[]));
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
/// its bytes: taken by address, returned through the caller's slot — however
/// the optional is spelled.
#[test]
fn a_body_over_optionals_held_as_bytes_takes_and_returns_them_by_address() {
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
    for optional in [
        TypeKind::Option(Box::new(ty(TypeKind::I32))),
        TypeKind::Option(Box::new(ty(point()))),
        written_option(TypeKind::I32),
        written_option(point()),
    ] {
        let body = body_of(optional.clone(), &[optional.clone()]);
        let signature = body_signature(&body, CALL_CONV, PTR);
        assert_eq!(
            signature,
            signature_of(&[PTR, PTR], &[]),
            "{optional:?} is passed and returned by address"
        );
    }
}
