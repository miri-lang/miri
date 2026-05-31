// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::types::TypeKind;
use miri::codegen::cranelift::FunctionTranslator;
use miri::mir::{Constant, Operand};

use miri::ast::literal::Literal;
use miri::ast::types::Type;
use miri::error::syntax::Span;
use miri::mir::{Local, Place};

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

#[test]
fn operand_has_no_projection_recognizes_bare_locals_and_constants() {
    let bare = Operand::Copy(Place {
        local: Local(0),
        projection: Vec::new(),
    });
    assert!(FunctionTranslator::operand_has_no_projection(&bare));

    let projected = Operand::Copy(Place {
        local: Local(0),
        projection: vec![miri::mir::PlaceElem::Field(0)],
    });
    assert!(!FunctionTranslator::operand_has_no_projection(&projected));

    let constant = Operand::Constant(Box::new(Constant {
        span: Span::default(),
        ty: ty(TypeKind::Int),
        literal: Literal::Integer(miri::ast::literal::IntegerLiteral::I64(42)),
    }));
    assert!(FunctionTranslator::operand_has_no_projection(&constant));
}
