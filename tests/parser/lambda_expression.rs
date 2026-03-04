// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test};
use miri::ast::factory::{
    array, binary, block, boolean_literal, call, empty_statement, expression_statement, func,
    generic_type, guard, identifier, int_literal_expression, lambda, let_variable, parameter,
    return_statement, string_literal_expression, type_bool, type_custom, type_expr_non_null,
    type_function, type_int, type_string, unary, variable_statement,
};
use miri::ast::{opt_expr, BinaryOp, GuardOp, MemberVisibility, UnaryOp};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_gpu_async_lambda() {
    parser_error_test(
        "let f = gpu async fn (): 1",
        &SyntaxErrorKind::InvalidModifierCombination {
            combination: "async gpu".to_string(),
            reason: "GPU kernels are inherently asynchronous.".to_string(),
        },
    );
}

#[test]
fn test_parallel_lambda() {
    parser_test(
        "let f = parallel fn (): 1",
        vec![variable_statement(
            vec![let_variable(
                "f",
                None,
                opt_expr(
                    lambda()
                        .set_parallel()
                        .build_lambda(expression_statement(int_literal_expression(1))),
                ),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_async_parallel_lambda() {
    parser_error_test(
        "let f = async parallel fn (): 1",
        &SyntaxErrorKind::InvalidModifierCombination {
            combination: "async parallel".to_string(),
            reason: "Parallel functions represent a different execution model and cannot be async."
                .to_string(),
        },
    );
}

#[test]
fn test_lambda_with_generics() {
    parser_test(
        "let identity = fn<T>(x T) T: x",
        vec![variable_statement(
            vec![let_variable(
                "identity",
                None,
                opt_expr(
                    lambda()
                        .generics(vec![generic_type("T", None)])
                        .params(vec![parameter(
                            "x".into(),
                            type_expr_non_null(type_custom("T", None)),
                            None,
                            None,
                        )])
                        .return_type(type_expr_non_null(type_custom("T", None)))
                        .build_lambda(expression_statement(identifier("x"))),
                ),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_nested_lambdas() {
    parser_test(
        "let add = fn (x int): fn (y int): x + y",
        vec![variable_statement(
            vec![let_variable(
                "add",
                None,
                opt_expr(
                    lambda()
                        .params(vec![parameter(
                            "x".into(),
                            type_expr_non_null(type_int()),
                            None,
                            None,
                        )])
                        .build_lambda(expression_statement(
                            lambda()
                                .params(vec![parameter(
                                    "y".into(),
                                    type_expr_non_null(type_int()),
                                    None,
                                    None,
                                )])
                                .build_lambda(expression_statement(binary(
                                    identifier("x"),
                                    BinaryOp::Add,
                                    identifier("y"),
                                ))),
                        )),
                ),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_immediately_invoked_function_expression() {
    parser_test(
        "(fn (x int): x * 2)(10)",
        vec![expression_statement(call(
            lambda()
                .params(vec![parameter(
                    "x".into(),
                    type_expr_non_null(type_int()),
                    None,
                    None,
                )])
                .build_lambda(expression_statement(binary(
                    identifier("x"),
                    BinaryOp::Mul,
                    int_literal_expression(2),
                ))),
            vec![int_literal_expression(10)],
        ))],
    );
}

#[test]
fn test_function_type_as_return_type() {
    parser_test(
        "fn counter() fn() int: fn() int: 1",
        vec![func("counter")
            .return_type(type_expr_non_null(type_function(
                None,
                vec![],
                opt_expr(type_expr_non_null(type_int())),
            )))
            .build(expression_statement(
                lambda()
                    .return_type(type_expr_non_null(type_int()))
                    .build_lambda(expression_statement(int_literal_expression(1))),
            ))],
    );
}

#[test]
fn test_lambda_with_empty_body() {
    parser_error_test("let no_op = fn ():", &SyntaxErrorKind::UnexpectedEOF);
}

#[test]
fn test_error_lambda_with_visibility_modifier() {
    // Lambdas are expressions and cannot have visibility modifiers.
    parser_error_test(
        "let f = public fn ()",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".to_string(),
            found: "public".to_string(),
        },
    );
}

#[test]
fn test_lambda_assignment() {
    parser_test(
        "let f = fn(): 1",
        vec![variable_statement(
            vec![let_variable(
                "f",
                None,
                opt_expr(lambda().build_lambda(expression_statement(int_literal_expression(1)))),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_error_lambda_missing_parameter_parens() {
    // TODO: this should actually be allowed at some point.
    parser_error_test(
        "let f = fn : 1",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "(".to_string(),
            found: ":".to_string(),
        },
    );
}

#[test]
fn test_error_lambda_with_statement_in_inline_body() {
    // An inline body must be an expression, not a statement like `let`.
    parser_error_test(
        "let f = fn (): let x = 1",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".to_string(),
            found: "let".to_string(),
        },
    );
}

#[test]
fn test_lambda_with_parameter_guard() {
    parser_test(
        "let check = fn (x int > 0): x",
        vec![variable_statement(
            vec![let_variable(
                "check",
                None,
                opt_expr(
                    lambda()
                        .params(vec![parameter(
                            "x".into(),
                            type_expr_non_null(type_int()),
                            opt_expr(guard(GuardOp::GreaterThan, int_literal_expression(0))),
                            None,
                        )])
                        .build_lambda(expression_statement(identifier("x"))),
                ),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_lambda_returned_from_function() {
    parser_test(
        "
fn get_adder()
    return fn (a int, b int): a + b
",
        vec![
            func("get_adder").build(block(vec![return_statement(opt_expr(
                lambda()
                    .params(vec![
                        parameter("a".into(), type_expr_non_null(type_int()), None, None),
                        parameter("b".into(), type_expr_non_null(type_int()), None, None),
                    ])
                    .build_lambda(expression_statement(binary(
                        identifier("a"),
                        BinaryOp::Add,
                        identifier("b"),
                    ))),
            ))])),
        ],
    );
}

#[test]
fn test_mutiline_lambda_as_parameter() {
    parser_test(
        "
func(
    fn (a int, b int): a + b,
    fn (c int, d int, e int)
        let x = c + d + e
        print(x)
        return x
,
    [
        6, 7, 8
    ], 'Some string'
)
",
        vec![expression_statement(call(
            identifier("func"),
            vec![
                lambda()
                    .params(vec![
                        parameter("a".into(), type_expr_non_null(type_int()), None, None),
                        parameter("b".into(), type_expr_non_null(type_int()), None, None),
                    ])
                    .build_lambda(expression_statement(binary(
                        identifier("a"),
                        BinaryOp::Add,
                        identifier("b"),
                    ))),
                lambda()
                    .params(vec![
                        parameter("c".into(), type_expr_non_null(type_int()), None, None),
                        parameter("d".into(), type_expr_non_null(type_int()), None, None),
                        parameter("e".into(), type_expr_non_null(type_int()), None, None),
                    ])
                    .build_lambda(block(vec![
                        variable_statement(
                            vec![let_variable(
                                "x",
                                None,
                                opt_expr(binary(
                                    binary(identifier("c"), BinaryOp::Add, identifier("d")),
                                    BinaryOp::Add,
                                    identifier("e"),
                                )),
                            )],
                            MemberVisibility::Public,
                        ),
                        expression_statement(call(identifier("print"), vec![identifier("x")])),
                        return_statement(opt_expr(identifier("x"))),
                    ])),
                array(
                    vec![
                        int_literal_expression(6),
                        int_literal_expression(7),
                        int_literal_expression(8),
                    ],
                    Box::new(int_literal_expression(3)),
                ),
                string_literal_expression("Some string"),
            ],
        ))],
    );
}

#[test]
fn test_async_iife_with_await() {
    // An immediately-invoked async lambda that is awaited.
    // This tests the precedence of `await` vs. `()`.
    parser_test(
        "await (async fn(): 1)()",
        vec![expression_statement(unary(
            UnaryOp::Await,
            call(
                lambda()
                    .set_async()
                    .build_lambda(expression_statement(int_literal_expression(1))),
                vec![],
            ),
        ))],
    );
}

#[test]
fn test_lambda_with_empty_block_body() {
    parser_test(
        "
let f = fn()
    // empty body
",
        vec![variable_statement(
            vec![let_variable(
                "f",
                None,
                opt_expr(lambda().build_lambda(empty_statement())),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_error_return_in_inline_lambda() {
    // An inline lambda body must be a single expression. `return` is a statement.
    parser_error_test(
        "let f = fn(): return 1",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".to_string(),
            found: "return".to_string(),
        },
    );
}

#[test]
fn test_error_lambda_with_misplaced_modifier() {
    // Modifiers like `async` must come before `fn`.
    parser_error_test(
        "let f = fn async (): 1",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "(".to_string(),
            found: "async".to_string(),
        },
    );
}

#[test]
fn test_lambda_with_default_parameter_values() {
    parser_test(
        "let f = fn (a int = 10, b bool = true): a",
        vec![variable_statement(
            vec![let_variable(
                "f",
                None,
                opt_expr(
                    lambda()
                        .params(vec![
                            parameter(
                                "a".into(),
                                type_expr_non_null(type_int()),
                                None,
                                opt_expr(int_literal_expression(10)),
                            ),
                            parameter(
                                "b".into(),
                                type_expr_non_null(type_bool()),
                                None,
                                opt_expr(boolean_literal(true)),
                            ),
                        ])
                        .build_lambda(expression_statement(identifier("a"))),
                ),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_lambda_with_keyword_as_parameter_name() {
    parser_error_test(
        "let f = fn (let int): let",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "let".to_string(),
        },
    );
}

#[test]
fn test_lambda_with_trailing_comma_in_parameters() {
    parser_test(
        "let f = fn (a int, b String,): a",
        vec![variable_statement(
            vec![let_variable(
                "f",
                None,
                opt_expr(
                    lambda()
                        .params(vec![
                            parameter("a".into(), type_expr_non_null(type_int()), None, None),
                            parameter("b".into(), type_expr_non_null(type_string()), None, None),
                        ])
                        .build_lambda(expression_statement(identifier("a"))),
                ),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_lambda_with_block_body_assignment() {
    parser_test(
        "
let f = fn ()
    let x = 1
    return x
",
        vec![variable_statement(
            vec![let_variable(
                "f",
                None,
                opt_expr(lambda().build_lambda(block(vec![
                    variable_statement(
                        vec![let_variable("x", None, opt_expr(int_literal_expression(1)))],
                        MemberVisibility::Public,
                    ),
                    return_statement(opt_expr(identifier("x"))),
                ]))),
            )],
            MemberVisibility::Public,
        )],
    );
}
