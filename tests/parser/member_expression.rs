// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_member_expression() {
    parser_test("
obj.prop
", vec![
        expression_statement(
            member(identifier("obj".into()), identifier("prop").into())
        )
    ]);
}

#[test]
fn test_assign_to_member_expression() {
    parser_test("
obj.prop = 1
", vec![
        expression_statement(
            assign(
                lhs_member(identifier("obj".into()), identifier("prop").into()),
                AssignmentOp::Assign,
                int_literal_expression(1)
            )
        )
    ]);
}

#[test]
fn test_assign_to_index_expression() {
    parser_test("
obj['prop'] = 1
", vec![
        expression_statement(
            assign(
                lhs_index(identifier("obj".into()), string_literal("prop".into())),
                AssignmentOp::Assign,
                int_literal_expression(1)
            )
        )
    ]);
}

#[test]
fn test_assign_to_chained_member_expression() {
    parser_test("
obj.a.b['prop'][0] = 1.0
", vec![
        expression_statement(
            assign(
                lhs_index(
                    index(
                        member(
                            member(
                                identifier("obj".into()),
                                identifier("a".into())
                            ),
                            identifier("b".into())
                        ),
                        string_literal("prop".into())
                    ),
                    int_literal_expression(0)
                ),
                AssignmentOp::Assign,
                float32_literal(1.0)
            )
        )
    ]);
}

#[test]
fn test_chained_calls_and_member_access() {
    // `a.b(c).d` should parse as `((a.b)(c)).d`
    parser_test("a.b(c).d", vec![
        expression_statement(
            member(
                call(
                    member(identifier("a"), identifier("b")),
                    vec![identifier("c")]
                ),
                identifier("d")
            )
        )
    ]);
}
