// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_lowering_binary_op_test;
use miri::mir::BinOp;

#[test]
fn test_add() {
    mir_lowering_binary_op_test("fn main(): 1 + 2", BinOp::Add);
}

#[test]
fn test_sub() {
    mir_lowering_binary_op_test("fn main(): 5 - 3", BinOp::Sub);
}

#[test]
fn test_mul() {
    mir_lowering_binary_op_test("fn main(): 2 * 3", BinOp::Mul);
}

#[test]
fn test_div() {
    mir_lowering_binary_op_test("fn main(): 10 / 2", BinOp::Div);
}

#[test]
fn test_mod() {
    mir_lowering_binary_op_test("fn main(): 10 % 3", BinOp::Rem);
}

#[test]
fn test_eq() {
    mir_lowering_binary_op_test("fn main(): 1 == 1", BinOp::Eq);
}

#[test]
fn test_ne() {
    mir_lowering_binary_op_test("fn main(): 1 != 2", BinOp::Ne);
}

#[test]
fn test_lt() {
    mir_lowering_binary_op_test("fn main(): 1 < 2", BinOp::Lt);
}

#[test]
fn test_le() {
    mir_lowering_binary_op_test("fn main(): 1 <= 2", BinOp::Le);
}

#[test]
fn test_gt() {
    mir_lowering_binary_op_test("fn main(): 2 > 1", BinOp::Gt);
}

#[test]
fn test_ge() {
    mir_lowering_binary_op_test("fn main(): 2 >= 1", BinOp::Ge);
}

#[test]
fn test_chained_additions() {
    mir_lowering_binary_op_test("fn main(): 1 + 2 + 3 + 4 + 5", BinOp::Add);
}

#[test]
fn test_chained_mixed_operations() {
    mir_lowering_binary_op_test("fn main(): 1 + 2 * 3 - 4 / 2", BinOp::Add);
}

#[test]
fn test_deeply_nested_parentheses() {
    mir_lowering_binary_op_test("fn main(): ((((1 + 2))))", BinOp::Add);
}

#[test]
fn test_binary_op_with_negative() {
    mir_lowering_binary_op_test("fn main(): -1 + -2", BinOp::Add);
}

#[test]
fn test_comparison_chain() {
    mir_lowering_binary_op_test("fn main(): 1 < 2 == true", BinOp::Lt);
}

#[test]
fn test_bitwise_and() {
    mir_lowering_binary_op_test("fn main(): 5 & 3", BinOp::BitAnd);
}

#[test]
fn test_max_int_addition() {
    mir_lowering_binary_op_test("fn main(): 2147483647 + 0", BinOp::Add);
}

#[test]
fn test_zero_division_expression() {
    mir_lowering_binary_op_test("fn main(): 10 / 1", BinOp::Div);
}

#[test]
fn test_modulo_with_one() {
    mir_lowering_binary_op_test("fn main(): 10 % 1", BinOp::Rem);
}
