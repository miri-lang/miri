// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::ast_factory::*;
use super::utils::*;


#[test]
fn test_return_statement() {
    parser_test("
return 42
", vec![
        return_statement(opt_expr(int_literal_expression(42)))
    ]);
}

#[test]
fn test_return_statement_with_expression() {
    parser_test("
return 42 + x
", vec![
        return_statement(opt_expr(binary(int_literal_expression(42), BinaryOp::Add, identifier("x".into()))))
    ]);
}

#[test]
fn test_empty_return_statement() {
    parser_test("
return
", vec![
        return_statement(None)
    ]);
}
