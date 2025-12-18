// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::*;
use miri::ast_factory::*;
use miri::syntax_error::SyntaxErrorKind;

#[test]
fn test_list_literal_assignment() {
    parser_test(
        "let arr = [1, 2, 3]",
        vec![variable_statement(
            vec![let_variable(
                "arr",
                None,
                opt_expr(list(vec![
                    int_literal_expression(1),
                    int_literal_expression(2),
                    int_literal_expression(3),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_list_index_access() {
    parser_test(
        "print(arr[0])",
        vec![expression_statement(call(
            identifier("print"),
            vec![index(identifier("arr"), int_literal_expression(0))],
        ))],
    );
}

#[test]
fn test_for_loop_over_list_literal() {
    parser_test(
        "
for el in [1, 2, 3]
    print(el)
",
        vec![for_statement(
            vec![let_variable("el", None, None)],
            range(
                list(vec![
                    int_literal_expression(1),
                    int_literal_expression(2),
                    int_literal_expression(3),
                ]),
                None,
                RangeExpressionType::IterableObject,
            ),
            block(vec![expression_statement(call(
                identifier("print"),
                vec![identifier("el")],
            ))]),
        )],
    );
}

#[test]
fn test_method_call_on_list_literal() {
    parser_test(
        "[1, 2, 3].each(fn (el int): print(el))",
        vec![expression_statement(call(
            member(
                list(vec![
                    int_literal_expression(1),
                    int_literal_expression(2),
                    int_literal_expression(3),
                ]),
                identifier("each"),
            ),
            vec![lambda()
                .params(vec![parameter("el".into(), typ(Type::Int), None, None)])
                .build_lambda(expression_statement(call(
                    identifier("print"),
                    vec![identifier("el")],
                )))],
        ))],
    );
}

#[test]
fn test_list_of_lambdas() {
    parser_test(
        "let funcs = [fn (): 1, fn (): 2]",
        vec![variable_statement(
            vec![let_variable(
                "funcs",
                None,
                opt_expr(list(vec![
                    lambda().build_lambda(expression_statement(int_literal_expression(1))),
                    lambda().build_lambda(expression_statement(int_literal_expression(2))),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_empty_list() {
    parser_test(
        "let empty = []",
        vec![variable_statement(
            vec![let_variable("empty", None, opt_expr(list(vec![])))],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_list_with_trailing_comma() {
    parser_test(
        "let arr = [1, 2,]",
        vec![variable_statement(
            vec![let_variable(
                "arr",
                None,
                opt_expr(list(vec![
                    int_literal_expression(1),
                    int_literal_expression(2),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_nested_lists() {
    parser_test(
        "let matrix = [[1, 2], [3, 4]]",
        vec![variable_statement(
            vec![let_variable(
                "matrix",
                None,
                opt_expr(list(vec![
                    list(vec![int_literal_expression(1), int_literal_expression(2)]),
                    list(vec![int_literal_expression(3), int_literal_expression(4)]),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_nested_lists_multiline() {
    parser_test(
        "
let matrix = [
    [
        1,
        2
    ],
    [
        3, 4
    ],
    [5],
[6,7,8]
]
",
        vec![variable_statement(
            vec![let_variable(
                "matrix",
                None,
                opt_expr(list(vec![
                    list(vec![int_literal_expression(1), int_literal_expression(2)]),
                    list(vec![int_literal_expression(3), int_literal_expression(4)]),
                    list(vec![int_literal_expression(5)]),
                    list(vec![
                        int_literal_expression(6),
                        int_literal_expression(7),
                        int_literal_expression(8),
                    ]),
                ])),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_error_unclosed_list() {
    parser_error_test("let arr = [1, 2", &SyntaxErrorKind::UnexpectedEOF);
}

#[test]
fn test_error_list_missing_comma() {
    parser_error_test(
        "let arr = [1 2]",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "]".to_string(),
            found: "int".to_string(),
        },
    );
}

#[test]
fn test_list_index_precedence() {
    // Index access on a literal has higher precedence than binary operators.
    // This should parse as `([1, 2, 3][0]) + 1`.
    parser_test(
        "
[1, 2, 3][0] + 1
",
        vec![expression_statement(binary(
            index(
                list(vec![
                    int_literal_expression(1),
                    int_literal_expression(2),
                    int_literal_expression(3),
                ]),
                int_literal_expression(0),
            ),
            BinaryOp::Add,
            int_literal_expression(1),
        ))],
    );
}
