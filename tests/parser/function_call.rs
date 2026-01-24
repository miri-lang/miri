// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::BinaryOp;
use miri::ast::UnaryOp;
use miri::error::syntax::SyntaxErrorKind;

use super::utils::{parser_error_test, parser_test};
use miri::ast::factory::{
    binary, boolean_literal, call, expression_statement, identifier, index, int_literal_expression,
    list, map, member, set, string_literal_expression, unary,
};

#[test]
fn test_function_call() {
    parser_test(
        "
print(\"Hello\")
",
        vec![expression_statement(call(
            identifier("print"),
            vec![string_literal_expression("Hello".into())],
        ))],
    );
}

#[test]
fn test_function_call_without_arguments() {
    parser_test(
        "
func()
",
        vec![expression_statement(call(identifier("func"), vec![]))],
    );
}

#[test]
fn test_chained_function_call() {
    parser_test(
        "
func(0)()
",
        vec![expression_statement(call(
            call(identifier("func"), vec![int_literal_expression(0)]),
            vec![],
        ))],
    );
}

#[test]
fn test_member_function_call() {
    parser_test(
        "
coordinates.compute(x, y, z)
",
        vec![expression_statement(call(
            member(identifier("coordinates"), identifier("compute")),
            vec![identifier("x"), identifier("y"), identifier("z")],
        ))],
    );
}

#[test]
fn test_function_call_with_complex_arguments() {
    parser_test(
        "
my_func(1 + 2, another_func(), true)
",
        vec![expression_statement(call(
            identifier("my_func"),
            vec![
                binary(
                    int_literal_expression(1),
                    BinaryOp::Add,
                    int_literal_expression(2),
                ),
                call(identifier("another_func"), vec![]),
                boolean_literal(true),
            ],
        ))],
    );
}

#[test]
fn test_complex_function_call_multiline() {
    parser_test(
        "
my_func(
    another_func(
        {'set', 'of', 'strings'},
        [
            0,
                1,
                2
        ], {
        'a': 10,
        'b': 20
    })
)
",
        vec![expression_statement(call(
            identifier("my_func"),
            vec![call(
                identifier("another_func"),
                vec![
                    set(vec![
                        string_literal_expression("set"),
                        string_literal_expression("of"),
                        string_literal_expression("strings"),
                    ]),
                    list(vec![
                        int_literal_expression(0),
                        int_literal_expression(1),
                        int_literal_expression(2),
                    ]),
                    map(vec![
                        (string_literal_expression("a"), int_literal_expression(10)),
                        (string_literal_expression("b"), int_literal_expression(20)),
                    ]),
                ],
            )],
        ))],
    );
}

#[test]
fn test_call_on_indexed_expression() {
    parser_test(
        "
my_array_of_funcs[0]()
",
        vec![expression_statement(call(
            index(identifier("my_array_of_funcs"), int_literal_expression(0)),
            vec![],
        ))],
    );
}

#[test]
fn test_call_precedence_with_operators() {
    // Function calls have higher precedence than unary and binary operators.
    // `not func()` should be `not (func())`
    // `func() + 1` should be `(func()) + 1`
    parser_test(
        "not my_func() + 1",
        vec![expression_statement(binary(
            unary(UnaryOp::Not, call(identifier("my_func"), vec![])),
            BinaryOp::Add,
            int_literal_expression(1),
        ))],
    );
}

#[test]
fn test_function_call_with_trailing_comma() {
    parser_test(
        "my_func(a, b,)",
        vec![expression_statement(call(
            identifier("my_func"),
            vec![
                identifier("a"),
                identifier("b"),
                // Trailing comma is allowed
            ],
        ))],
    );
}

#[test]
fn test_error_on_unclosed_function_call() {
    parser_error_test("my_func(a, b", &SyntaxErrorKind::UnexpectedEOF);
}

#[test]
fn test_error_on_missing_comma() {
    parser_error_test(
        "my_func(a b)",
        &SyntaxErrorKind::UnexpectedToken {
            expected: ")".to_string(),
            found: "identifier".to_string(),
        },
    );
}
