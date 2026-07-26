// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::literal::{IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::body::Body;
use miri::mir::operand::{Constant, Operand};
use miri::mir::place::{Place, PlaceElem};
use miri::mir::types::MirType;
use miri::mir::{ExecutionModel, Local, LocalDecl};

fn new_local_with_mir_ty(body: &mut Body, mir_ty: MirType) -> Local {
    let span = Span::default();
    let mut decl = LocalDecl::new(Type::new(TypeKind::Int, span), span);
    decl.mir_ty = mir_ty;
    body.new_local(decl)
}

#[test]
fn test_ty_projected_constant() {
    let span = Span::default();
    let c = Constant {
        span,
        ty: Type::new(TypeKind::I32, span),
        literal: Literal::Integer(IntegerLiteral::I32(42)),
    };

    let operand = Operand::Constant(Box::new(c));
    let body = Body::new(0, span, ExecutionModel::Cpu);

    assert_eq!(operand.ty_projected(&body), Some(MirType::I32));
}

#[test]
fn test_ty_projected_bare_local() {
    let span = Span::default();
    let mut body = Body::new(0, span, ExecutionModel::Cpu);
    let local = body.new_local(LocalDecl::new(Type::new(TypeKind::I32, span), span));

    let operand = Operand::Copy(Place::new(local));
    assert_eq!(operand.ty_projected(&body), Some(MirType::I32));
}

#[test]
fn test_ty_projected_list_index() {
    let span = Span::default();
    let mut body = Body::new(0, span, ExecutionModel::Cpu);
    let local = new_local_with_mir_ty(&mut body, MirType::List(Box::new(MirType::I32)));
    let index_local = body.new_local(LocalDecl::new(Type::new(TypeKind::I32, span), span));

    let place = Place {
        local,
        projection: vec![PlaceElem::Index(index_local)],
    };
    let operand = Operand::Copy(place);
    assert_eq!(operand.ty_projected(&body), Some(MirType::I32));
}

#[test]
fn test_ty_projected_tuple_field() {
    let span = Span::default();
    let mut body = Body::new(0, span, ExecutionModel::Cpu);
    let local = new_local_with_mir_ty(
        &mut body,
        MirType::Tuple(vec![MirType::I32, MirType::String]),
    );

    let place = Place {
        local,
        projection: vec![PlaceElem::Field(0)],
    };
    let operand = Operand::Copy(place);
    assert_eq!(operand.ty_projected(&body), Some(MirType::I32));
}

#[test]
fn test_ty_projected_deref_returns_none() {
    let span = Span::default();
    let mut body = Body::new(0, span, ExecutionModel::Cpu);
    let local = body.new_local(LocalDecl::new(Type::new(TypeKind::RawPtr, span), span));

    let place = Place {
        local,
        projection: vec![PlaceElem::Deref],
    };
    let operand = Operand::Copy(place);
    assert_eq!(operand.ty_projected(&body), None);
}

#[test]
fn test_ty_projected_custom_struct_field() {
    let span = Span::default();
    let mut body = Body::new(0, span, ExecutionModel::Cpu);
    let i32_type = Type::new(TypeKind::I32, span);
    body.field_types
        .insert("Point".to_string(), vec![i32_type.clone(), i32_type]);
    let local = new_local_with_mir_ty(&mut body, MirType::Custom("Point".to_string()));

    let place = Place {
        local,
        projection: vec![PlaceElem::Field(0)],
    };
    let operand = Operand::Copy(place);
    assert_eq!(operand.ty_projected(&body), Some(MirType::I32));
}
