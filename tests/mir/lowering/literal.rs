// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::super::utils::{expect_assignment, lower_code};
use miri::mir::{Operand, Rvalue};

#[test]
fn test_lower_literal() {
    let source = "fn main(): 42";
    let body = lower_code(source);

    // Check that we have a temp assigned to 42
    // _0: return
    // _1: temp
    // bb0: { _1 = const 42; return; }

    assert_eq!(body.local_decls.len(), 1); // _0 only (direct assignment)
    let bb0 = &body.basic_blocks[0];
    assert_eq!(bb0.statements.len(), 1);

    let (place, rvalue) = expect_assignment(&bb0.statements[0]);
    assert_eq!(place.local.0, 0); // Assign to return place
    match rvalue {
        Rvalue::Use(Operand::Constant(c)) => match c.literal {
            miri::ast::literal::Literal::Integer(miri::ast::literal::IntegerLiteral::I8(42)) => {}
            _ => panic!("Expected integer literal 42, got {:?}", c.literal),
        },
        _ => panic!("Expected Use(Constant)"),
    }
}
