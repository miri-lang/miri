// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_lowering_unary_op_test;
use miri::mir::UnOp;

#[test]
fn test_neg() {
    mir_lowering_unary_op_test("fn main(): -1", UnOp::Neg);
}

#[test]
fn test_not() {
    mir_lowering_unary_op_test("fn main(): not true", UnOp::Not);
}

#[test]
fn test_double_negation() {
    mir_lowering_unary_op_test("fn main(): --1", UnOp::Neg);
}

#[test]
fn test_double_not() {
    mir_lowering_unary_op_test("fn main(): not not true", UnOp::Not);
}

#[test]
fn test_negation_with_parentheses() {
    mir_lowering_unary_op_test("fn main(): -(1 + 2)", UnOp::Neg);
}

#[test]
fn test_not_with_comparison() {
    mir_lowering_unary_op_test("fn main(): not (1 < 2)", UnOp::Not);
}

#[test]
fn test_negation_of_variable() {
    mir_lowering_unary_op_test(
        "
fn main()
    let x = 5
    -x
",
        UnOp::Neg,
    );
}
