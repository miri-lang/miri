// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::lower_code;
use miri::mir::{Rvalue, UnOp};

fn assert_unary_op(source: &str, expected_op: UnOp) {
    let body = lower_code(source);
    let found = body.basic_blocks.iter().any(|bb| {
        bb.statements.iter().any(|stmt| {
            if let miri::mir::StatementKind::Assign(_, Rvalue::UnaryOp(op, _)) = &stmt.kind {
                *op == expected_op
            } else {
                false
            }
        })
    });
    assert!(found, "Expected {:?} operation in MIR", expected_op);
}

#[test]
fn test_neg() {
    assert_unary_op("fn main(): -1", UnOp::Neg);
}

#[test]
fn test_not() {
    assert_unary_op("fn main(): not true", UnOp::Not);
}
