// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::super::utils::lower_code;
use miri::mir::{BinOp, Operand, Rvalue};

#[test]
fn test_lower_binary_expression_body() {
    let source = "fn main(): 1 + 2";
    let body = lower_code(source);

    let bb0 = &body.basic_blocks[0];

    // Find the Add operation
    let mut found_add = false;
    for stmt in &bb0.statements {
        if let miri::mir::StatementKind::Assign(_, Rvalue::BinaryOp(op, lhs, rhs)) = &stmt.kind {
            if *op == BinOp::Add {
                found_add = true;
                match (&**lhs, &**rhs) {
                    (Operand::Constant(_), Operand::Constant(_)) => {}
                    _ => panic!("Expected constants for Add operands"),
                }
            }
        }
    }
    assert!(found_add, "Did not find Add operation");
}
