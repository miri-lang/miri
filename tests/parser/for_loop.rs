// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_for_loop() {
    parse_for_test("
for x in 1..=5
    y = x
",
    vec![
        let_variable("x".into(), None, None)
    ],
    range(
        int_literal_expression(1),
        opt_expr(int_literal_expression(5)),
        RangeExpressionType::Inclusive
    ),
    block(vec![
        expression_statement(
            assign(
                lhs_identifier("y"),
                AssignmentOp::Assign,
                identifier("x".into())
            )
        )
    ])
    );
}

#[test]
fn test_for_loop_inline() {
    parse_for_test("
for x in 1..5: y = x
",
    vec![
        let_variable("x".into(), None, None)
    ],
    range(
        int_literal_expression(1),
        opt_expr(int_literal_expression(5)),
        RangeExpressionType::Exclusive
    ),
    expression_statement(
        assign(
            lhs_identifier("y"),
            AssignmentOp::Assign,
            identifier("x".into())
        )
    )
    );
}

#[test]
fn test_for_loop_hashmap() {
    parse_for_test("
for k, v in hash: y = k + v
",
    vec![
        let_variable("k".into(), None, None),
        let_variable("v".into(), None, None)
    ],
    range(identifier("hash".into()), None, RangeExpressionType::IterableObject),
    expression_statement(
        assign(
            lhs_identifier("y"),
            AssignmentOp::Assign,
            binary(
                identifier("k".into()),
                BinaryOp::Add,
                identifier("v".into())
            )
        )
    )
    );
}

#[test]
fn test_for_loop_string() {
    parse_for_test("
for ch in \"hello\": y = ch
",
    vec![
        let_variable("ch".into(), None, None),
    ],
    range(
        string_literal("hello"),
        None,
        RangeExpressionType::IterableObject
    ),
    expression_statement(
        assign(
            lhs_identifier("y"),
            AssignmentOp::Assign,
            identifier("ch".into())
        )
    )
    );
}

#[test]
fn test_nested_for_loop() {
    parse_for_test("
for i in 1..3
    for c in \"ab\"
        // nested body
",
        vec![let_variable("i".into(), None, None)],
        range(int_literal_expression(1), opt_expr(int_literal_expression(3)), RangeExpressionType::Exclusive),
        block(vec![
            for_statement(
                vec![let_variable("c".into(), None, None)],
                range(string_literal("ab"), None, RangeExpressionType::IterableObject),
                empty_statement()
            )
        ])
    );
}

#[test]
fn test_for_loop_nested_inline() {
    parse_for_test("
for i in 1..3: for c in \"ab\": // nested body
",
        vec![let_variable("i".into(), None, None)],
        range(int_literal_expression(1), opt_expr(int_literal_expression(3)), RangeExpressionType::Exclusive),
        for_statement(
                vec![let_variable("c".into(), None, None)],
                range(string_literal("ab"), None, RangeExpressionType::IterableObject),
                empty_statement()
            )
    );
}

#[test]
fn test_for_loop_with_typed_variable() {
    parse_for_test("
for i int in 1..=10: // do something
",
        vec![let_variable("i".into(), opt_expr(typ(Type::Int)), None)],
        range(int_literal_expression(1), opt_expr(int_literal_expression(10)), RangeExpressionType::Inclusive),
        empty_statement()
    );
}

#[test]
fn test_for_loop_with_empty_body() {
    parse_for_test("
for item in my_list
    // This loop is intentionally empty
",
        vec![let_variable("item".into(), None, None)],
        range(identifier("my_list".into()), None, RangeExpressionType::IterableObject),
        empty_statement()
    );
}

#[test]
fn test_error_for_loop_variable_with_initializer() {
    // The parser should reject initializers on loop variables.
    parse_error_test(
        "for x = 10 in 1..5",
        SyntaxErrorKind::UnexpectedToken {
            expected: "in".to_string(),
            found: "=".to_string(),
        }
    );
}

#[test]
fn test_error_for_loop_missing_in_keyword() {
    parse_error_test(
        "for x 1..5",
        SyntaxErrorKind::UnexpectedToken {
            expected: "in".to_string(),
            found: "int".to_string(),
        }
    );
}

#[test]
fn test_error_for_loop_without_body() {
    parse_error_test(
        "for x in (get_items())",
        SyntaxErrorKind::UnexpectedToken {
            expected: "a colon or an expression end".to_string(),
            found: "end of file".to_string(),
        }
    );
}

#[test]
fn test_error_invalid_item_in_for_loop_declaration() {
    // `for x, y+1 in items` is invalid because `y+1` is not a valid declaration target.
    parse_error_test(
        "for x, y + 1 in items",
        SyntaxErrorKind::UnexpectedToken {
            expected: "in".to_string(),
            found: "+".to_string(),
        }
    );
}
