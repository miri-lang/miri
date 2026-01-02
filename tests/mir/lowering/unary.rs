// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::super::utils::lower_code;
use miri::mir::{Operand, Rvalue, UnOp};

#[test]
fn test_lower_unary_expression() {
    let source = "fn main(): -1";
    let body = lower_code(source);

    let bb0 = &body.basic_blocks[0];

    // Find the Neg operation
    let mut found_neg = false;
    for stmt in &bb0.statements {
        if let miri::mir::StatementKind::Assign(_, Rvalue::UnaryOp(op, operand)) = &stmt.kind {
            if *op == UnOp::Neg {
                found_neg = true;
                match &**operand {
                    Operand::Constant(_) => {}
                    _ => panic!("Expected constant for Neg operand"),
                }
            }
        }
    }
    assert!(found_neg, "Did not find Neg operation");
}
