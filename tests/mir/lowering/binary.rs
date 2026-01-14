// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::lowering_test_binary_op;
use miri::mir::BinOp;

#[test]
fn test_add() {
    lowering_test_binary_op("fn main(): 1 + 2", BinOp::Add);
}

#[test]
fn test_sub() {
    lowering_test_binary_op("fn main(): 5 - 3", BinOp::Sub);
}

#[test]
fn test_mul() {
    lowering_test_binary_op("fn main(): 2 * 3", BinOp::Mul);
}

#[test]
fn test_div() {
    lowering_test_binary_op("fn main(): 10 / 2", BinOp::Div);
}

#[test]
fn test_mod() {
    lowering_test_binary_op("fn main(): 10 % 3", BinOp::Rem);
}

#[test]
fn test_eq() {
    lowering_test_binary_op("fn main(): 1 == 1", BinOp::Eq);
}

#[test]
fn test_ne() {
    lowering_test_binary_op("fn main(): 1 != 2", BinOp::Ne);
}

#[test]
fn test_lt() {
    lowering_test_binary_op("fn main(): 1 < 2", BinOp::Lt);
}

#[test]
fn test_le() {
    lowering_test_binary_op("fn main(): 1 <= 2", BinOp::Le);
}

#[test]
fn test_gt() {
    lowering_test_binary_op("fn main(): 2 > 1", BinOp::Gt);
}

#[test]
fn test_ge() {
    lowering_test_binary_op("fn main(): 2 >= 1", BinOp::Ge);
}
