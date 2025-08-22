// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_gpu_async_lambda() {
    parse_test("let f = gpu async fn (): 1", vec![
        variable_statement(vec![
            let_variable(
                "f",
                None,
                opt_expr(
                    lambda().set_gpu().set_async().build_lambda(
                        expression_statement(int_literal_expression(1))
                    )
                )
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_lambda_with_generics() {
    parse_test("let identity = fn<T>(x T) T: x", vec![
        variable_statement(vec![
            let_variable(
                "identity",
                None,
                opt_expr(
                    lambda()
                        .generics(vec![generic_type("T", None)])
                        .params(vec![parameter("x".into(), opt_expr(typ(Type::Custom("T".into(), None))), None)])
                        .return_type(typ(Type::Custom("T".into(), None)))
                        .build_lambda(expression_statement(identifier("x")))
                )
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_nested_lambdas() {
    parse_test("let add = fn (x int): fn (y int): x + y", vec![
        variable_statement(vec![
            let_variable(
                "add",
                None,
                opt_expr(
                    lambda()
                        .params(vec![parameter("x".into(), opt_expr(typ(Type::Int)), None)])
                        .build_lambda(
                            expression_statement(
                                lambda()
                                    .params(vec![parameter("y".into(), opt_expr(typ(Type::Int)), None)])
                                    .build_lambda(expression_statement(
                                        binary(identifier("x"), BinaryOp::Add, identifier("y"))
                                    ))
                            )
                        )
                )
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_immediately_invoked_function_expression() {
    parse_test("(fn (x int): x * 2)(10)", vec![
        expression_statement(
            call(
                lambda()
                    .params(vec![parameter("x".into(), opt_expr(typ(Type::Int)), None)])
                    .build_lambda(expression_statement(
                        binary(identifier("x"), BinaryOp::Mul, int_literal_expression(2))
                    )),
                vec![int_literal_expression(10)]
            )
        )
    ]);
}

#[test]
fn test_function_type_as_return_type() {
    parse_test("fn counter() fn() int: fn() int: 1", vec![
        func("counter")
            .return_type(typ(Type::Function(None, vec![], opt_expr(typ(Type::Int)))))
            .build(expression_statement(
                lambda()
                    .return_type(typ(Type::Int))
                    .build_lambda(expression_statement(int_literal_expression(1)))
            ))
    ]);
}

#[test]
fn test_lambda_with_empty_body() {
    parse_error_test("let no_op = fn ():", SyntaxErrorKind::UnexpectedEOF);
}

#[test]
fn test_error_lambda_with_visibility_modifier() {
    // Lambdas are expressions and cannot have visibility modifiers.
    parse_error_test(
        "let f = public fn ()",
        SyntaxErrorKind::UnexpectedToken {
            expected: "literal, parenthesized expression, identifier, lambda, list, map or set".to_string(),
            found: "public".to_string(),
        }
    );
}

#[test]
fn test_lambda_assignment() {
    parse_test(
        "let f = fn(): 1",
        vec![
            variable_statement(vec![
                let_variable(
                    "f",
                    None,
                    opt_expr(lambda().build_lambda(expression_statement(int_literal_expression(1))))
                )
            ], MemberVisibility::Public)
        ]
    );
}

#[test]
fn test_error_lambda_missing_parameter_parens() {
    // TODO: this should actually be allowed at some point.
    parse_error_test(
        "let f = fn : 1",
        SyntaxErrorKind::UnexpectedToken {
            expected: "(".to_string(),
            found: ":".to_string(),
        }
    );
}

#[test]
fn test_error_lambda_with_statement_in_inline_body() {
    // An inline body must be an expression, not a statement like `let`.
    parse_error_test(
        "let f = fn (): let x = 1",
        SyntaxErrorKind::UnexpectedToken {
            expected: "literal, parenthesized expression, identifier, lambda, list, map or set".to_string(),
            found: "let".to_string(),
        }
    );
}

#[test]
fn test_lambda_with_parameter_guard() {
    parse_test("let check = fn (x int > 0): x", vec![
        variable_statement(vec![
            let_variable(
                "check",
                None,
                opt_expr(
                    lambda()
                        .params(vec![
                            parameter(
                                "x".into(),
                                opt_expr(typ(Type::Int)),
                                opt_expr(guard(GuardOp::GreaterThan, int_literal_expression(0)))
                            )
                        ])
                        .build_lambda(expression_statement(identifier("x")))
                )
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_lambda_returned_from_function() {
    parse_test("
fn get_adder()
    return fn (a, b): a + b
", vec![
        func("get_adder").build(
            block(vec![
                return_statement(opt_expr(
                    lambda()
                        .params(vec![
                            parameter("a".into(), None, None),
                            parameter("b".into(), None, None)
                        ])
                        .build_lambda(expression_statement(
                            binary(identifier("a"), BinaryOp::Add, identifier("b"))
                        ))
                ))
            ])
        )
    ]);
}

#[test]
fn test_mutiline_lambda_as_parameter() {
    parse_test("
func(
    fn (a, b): a + b,
    fn (c, d, e)
        let x = c + d + e
        print(x)
        return x
,
    [
        6, 7, 8
    ], 'Some string'
)
", vec![
        expression_statement(
            call(
                identifier("func"),
                vec![
                    lambda()
                        .params(vec![
                            parameter("a".into(), None, None),
                            parameter("b".into(), None, None)
                        ])
                        .build_lambda(expression_statement(
                            binary(identifier("a"), BinaryOp::Add, identifier("b"))
                        )),
                    lambda()
                        .params(vec![
                            parameter("c".into(), None, None),
                            parameter("d".into(), None, None),
                            parameter("e".into(), None, None)
                        ])
                        .build_lambda(block(vec![
                            variable_statement(vec![
                                let_variable("x", None,
                                    opt_expr(
                                        binary(
                                            binary(
                                                identifier("c"),
                                                BinaryOp::Add,
                                                identifier("d")
                                            ),
                                            BinaryOp::Add,
                                            identifier("e")
                                        )
                                     )
                                )],
                                MemberVisibility::Public
                            ),
                            expression_statement(call(identifier("print"), vec![identifier("x")])),
                            return_statement(opt_expr(identifier("x")))
                        ])),
                    list(vec![int_literal_expression(6), int_literal_expression(7), int_literal_expression(8)]),
                    string_literal("Some string")
                ]
            )
        )
    ]);
}

#[test]
fn test_async_iife_with_await() {
    // An immediately-invoked async lambda that is awaited.
    // This tests the precedence of `await` vs. `()`.
    parse_test("await (async fn(): 1)()", vec![
        expression_statement(
            unary(
                UnaryOp::Await,
                call(
                    lambda()
                        .set_async()
                        .build_lambda(expression_statement(int_literal_expression(1))),
                    vec![]
                )
            )
        )
    ]);
}

#[test]
fn test_lambda_with_empty_block_body() {
    parse_test("
let f = fn()
    // empty body
", vec![
        variable_statement(vec![
            let_variable(
                "f",
                None,
                opt_expr(lambda().build_lambda(empty_statement()))
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_error_return_in_inline_lambda() {
    // An inline lambda body must be a single expression. `return` is a statement.
    parse_error_test(
        "let f = fn(): return 1",
        SyntaxErrorKind::UnexpectedToken {
            expected: "literal, parenthesized expression, identifier, lambda, list, map or set".to_string(),
            found: "return".to_string(),
        }
    );
}

#[test]
fn test_error_lambda_with_misplaced_modifier() {
    // Modifiers like `async` must come before `fn`.
    parse_error_test(
        "let f = fn async (): 1",
        SyntaxErrorKind::UnexpectedToken {
            expected: "(".to_string(),
            found: "async".to_string(),
        }
    );
}
