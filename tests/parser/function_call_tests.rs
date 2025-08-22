// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_function_call() {
    parse_test("
print(\"Hello\")
", vec![
        expression_statement(
            call(
                identifier("print".into()),
                vec![string_literal("Hello".into())]
            )
        )
    ]);
}

#[test]
fn test_chained_function_call() {
    parse_test("
func(0)()
", vec![
        expression_statement(
            call(
                call(
                    identifier("func".into()),
                    vec![int_literal_expression(0)]
                ),
                vec![]
            )
        )
    ]);
}

#[test]
fn test_member_function_call() {
    parse_test("
coordinates.compute(x, y, z)
", vec![
        expression_statement(
            call(
                member(
                    identifier("coordinates".into()), 
                    identifier("compute".into())
                ),
                vec![identifier("x".into()), identifier("y".into()), identifier("z".into())]
            )
        )
    ]);
}
