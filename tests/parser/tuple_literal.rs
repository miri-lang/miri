// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use miri::ast_factory::*;
use super::utils::*;


#[test]
fn test_tuple_literal_assignment() {
    parser_test("let t = (:ok, 'Hello', 200)", vec![
        variable_statement(vec![
            let_variable("t", None, opt_expr(tuple(vec![
                symbol_literal("ok"),
                string_literal_expression("Hello"),
                int_literal_expression(200),
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_tuple_index_access() {
    parser_test("print(my_tuple[1])", vec![
        expression_statement(
            call(
                identifier("print"),
                vec![index(identifier("my_tuple"), int_literal_expression(1))]
            )
        )
    ]);
}

#[test]
fn test_for_loop_over_tuple_literal() {
    parser_test("
for el in (:ok, 200)
    print(el)
", vec![
        for_statement(
            vec![let_variable("el", None, None)],
            iter_obj(
                tuple(vec![symbol_literal("ok"), int_literal_expression(200)])
            ),
            block(vec![expression_statement(call(identifier("print"), vec![identifier("el")]))])
        )
    ]);
}

#[test]
fn test_method_call_on_tuple_literal() {
    parser_test("(:ok, 200).len()", vec![
        expression_statement(
            call(
                member(
                    tuple(vec![symbol_literal("ok"), int_literal_expression(200)]),
                    identifier("len")
                ),
                vec![]
            )
        )
    ]);
}

#[test]
fn test_tuple_of_lambdas() {
    parser_test("let funcs = (fn(): 1, fn(): 2)", vec![
        variable_statement(vec![
            let_variable("funcs", None, opt_expr(tuple(vec![
                lambda().build_lambda(expression_statement(int_literal_expression(1))),
                lambda().build_lambda(expression_statement(int_literal_expression(2))),
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_empty_tuple_unit_tuple() {
    parser_test("let unit = ()", vec![
        variable_statement(vec![
            let_variable("unit", None, opt_expr(tuple(vec![])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_single_item_tuple() {
    parser_test("let num = (42,)", vec![
        variable_statement(vec![
            let_variable("num", None, opt_expr(tuple(vec![int_literal_expression(42)])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_single_item_not_tuple() {
    parser_test("let num = (42 + 100)", vec![
        variable_statement(vec![
            let_variable("num", None, opt_expr(
                binary(int_literal_expression(42), BinaryOp::Add, int_literal_expression(100))
            ))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_assignment_to_nested_parenthesized_expression() {
    parser_test("let num = ((1))", vec![
        variable_statement(vec![
            let_variable(
                "num", 
                None, 
                opt_expr(int_literal_expression(1))
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_tuple_multiline() {
    parser_test("
let num = (
    1,
        2, 3,
4,
    5,
        6,
            7
)
", vec![
        variable_statement(vec![
            let_variable("num", None, opt_expr(tuple(vec![
                int_literal_expression(1),
                int_literal_expression(2),
                int_literal_expression(3),
                int_literal_expression(4),
                int_literal_expression(5),
                int_literal_expression(6),
                int_literal_expression(7)
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_single_item_tuple_with_map() {
    parser_test("let num = ({'a': 1},)", vec![
        variable_statement(vec![
            let_variable("num", None, opt_expr(tuple(vec![
                map(vec![
                    (string_literal_expression("a"), int_literal_expression(1))
                ])
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_single_item_tuple_with_member() {
    parser_test("let num = (obj.prop,)", vec![
        variable_statement(vec![
            let_variable("num", None, opt_expr(tuple(vec![
                member(identifier("obj"), identifier("prop"))
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_single_item_tuple_with_call() {
    parser_test("let num = (obj.func(),)", vec![
        variable_statement(vec![
            let_variable("num", None, opt_expr(tuple(vec![
                call(member(identifier("obj"), identifier("func")), vec![])
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_single_item_tuple_with_index() {
    parser_test("let num = (obj[0],)", vec![
        variable_statement(vec![
            let_variable("num", None, opt_expr(tuple(vec![
                index(identifier("obj"), int_literal_expression(0))
            ])))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_single_item_tuple_with_lambda() {
    // It makes no sense to have such a tuple, and supporting it
    // creates problems with expressions like (fn(): 1)()
    parser_test("let num = (fn(): 1)", vec![
        variable_statement(vec![
            let_variable("num", None, opt_expr(
                lambda().build_lambda(expression_statement(int_literal_expression(1)))
            ))
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_error_unclosed_tuple() {
    parser_error_test(
        "let t = (1, 2",
        &SyntaxErrorKind::UnexpectedEOF
    );
}

#[test]
fn test_error_tuple_missing_comma() {
    parser_error_test(
        "let t = (1 2)",
        &SyntaxErrorKind::UnexpectedToken {
            expected: ")".to_string(),
            found: "int".to_string(),
        }
    );
}

#[test]
fn test_tuple_index_precedence() {
    // Index access on a literal has higher precedence than binary operators.
    // This should parse as `((1, 2)[0]) + 3`.
    parser_test("(1, 2)[0] + 3", vec![
        expression_statement(
            binary(
                index(
                    tuple(vec![
                        int_literal_expression(1),
                        int_literal_expression(2),
                    ]),
                    int_literal_expression(0)
                ),
                BinaryOp::Add,
                int_literal_expression(3)
            )
        )
    ]);
}
