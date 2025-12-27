// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_for_loop() {
    for_statement_test(
        "
for x in 1..=5
    y = x
",
        vec![let_variable("x".into(), None, None)],
        range(
            int_literal_expression(1),
            opt_expr(int_literal_expression(5)),
            RangeExpressionType::Inclusive,
        ),
        block(vec![expression_statement(assign(
            lhs_identifier("y"),
            AssignmentOp::Assign,
            identifier("x".into()),
        ))]),
    );
}

#[test]
fn test_for_loop_inline() {
    for_statement_test(
        "
for x in 1..5: y = x
",
        vec![let_variable("x".into(), None, None)],
        range(
            int_literal_expression(1),
            opt_expr(int_literal_expression(5)),
            RangeExpressionType::Exclusive,
        ),
        expression_statement(assign(
            lhs_identifier("y"),
            AssignmentOp::Assign,
            identifier("x".into()),
        )),
    );
}

#[test]
fn test_for_loop_hashmap() {
    for_statement_test(
        "
for k, v in hash: y = k + v
",
        vec![
            let_variable("k".into(), None, None),
            let_variable("v".into(), None, None),
        ],
        range(
            identifier("hash".into()),
            None,
            RangeExpressionType::IterableObject,
        ),
        expression_statement(assign(
            lhs_identifier("y"),
            AssignmentOp::Assign,
            binary(
                identifier("k".into()),
                BinaryOp::Add,
                identifier("v".into()),
            ),
        )),
    );
}

#[test]
fn test_for_loop_string() {
    for_statement_test(
        "
for ch in \"hello\": y = ch
",
        vec![let_variable("ch".into(), None, None)],
        range(
            string_literal_expression("hello"),
            None,
            RangeExpressionType::IterableObject,
        ),
        expression_statement(assign(
            lhs_identifier("y"),
            AssignmentOp::Assign,
            identifier("ch".into()),
        )),
    );
}

#[test]
fn test_nested_for_loop() {
    for_statement_test(
        "
for i in 1..3
    for c in \"ab\"
        // nested body
",
        vec![let_variable("i".into(), None, None)],
        range(
            int_literal_expression(1),
            opt_expr(int_literal_expression(3)),
            RangeExpressionType::Exclusive,
        ),
        block(vec![for_statement(
            vec![let_variable("c".into(), None, None)],
            range(
                string_literal_expression("ab"),
                None,
                RangeExpressionType::IterableObject,
            ),
            empty_statement(),
        )]),
    );
}

#[test]
fn test_for_loop_nested_inline() {
    for_statement_test(
        "
for i in 1..3: for c in \"ab\": // nested body
",
        vec![let_variable("i".into(), None, None)],
        range(
            int_literal_expression(1),
            opt_expr(int_literal_expression(3)),
            RangeExpressionType::Exclusive,
        ),
        for_statement(
            vec![let_variable("c".into(), None, None)],
            range(
                string_literal_expression("ab"),
                None,
                RangeExpressionType::IterableObject,
            ),
            empty_statement(),
        ),
    );
}

#[test]
fn test_for_loop_with_typed_variable() {
    for_statement_test(
        "
for i int in 1..=10: // do something
",
        vec![let_variable(
            "i".into(),
            opt_expr(type_expr_non_null(type_int())),
            None,
        )],
        range(
            int_literal_expression(1),
            opt_expr(int_literal_expression(10)),
            RangeExpressionType::Inclusive,
        ),
        empty_statement(),
    );
}

#[test]
fn test_for_loop_with_empty_body() {
    for_statement_test(
        "
for item in my_list
    // This loop is intentionally empty
",
        vec![let_variable("item".into(), None, None)],
        range(
            identifier("my_list".into()),
            None,
            RangeExpressionType::IterableObject,
        ),
        empty_statement(),
    );
}

#[test]
fn test_error_for_loop_variable_with_initializer() {
    // The parser should reject initializers on loop variables.
    parser_error_test(
        "for x = 10 in 1..5",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "in".to_string(),
            found: "=".to_string(),
        },
    );
}

#[test]
fn test_error_for_loop_missing_in_keyword() {
    parser_error_test(
        "for x 1..5",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "in".to_string(),
            found: "int".to_string(),
        },
    );
}

#[test]
fn test_error_for_loop_without_body() {
    parser_error_test(
        "for x in (get_items())",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "a colon or an expression end".to_string(),
            found: "end of file".to_string(),
        },
    );
}

#[test]
fn test_error_invalid_item_in_for_loop_declaration() {
    // `for x, y+1 in items` is invalid because `y+1` is not a valid declaration target.
    parser_error_test(
        "for x, y + 1 in items",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "in".to_string(),
            found: "+".to_string(),
        },
    );
}

#[test]
fn test_for_loop_with_complex_iterable() {
    for_statement_test(
        "
for item in get_items()
    // body
",
        vec![let_variable("item", None, None)],
        range(
            call(identifier("get_items"), vec![]),
            None,
            RangeExpressionType::IterableObject,
        ),
        empty_statement(),
    );
}

#[test]
fn test_for_loop_with_break_and_continue() {
    for_statement_test(
        "
for i in 1..10
    if i % 2 == 0: continue
    if i == 7: break
",
        vec![let_variable("i", None, None)],
        range(
            int_literal_expression(1),
            opt_expr(int_literal_expression(10)),
            RangeExpressionType::Exclusive,
        ),
        block(vec![
            if_statement(
                binary(
                    binary(identifier("i"), BinaryOp::Mod, int_literal_expression(2)),
                    BinaryOp::Equal,
                    int_literal_expression(0),
                ),
                continue_statement(),
                None,
            ),
            if_statement(
                binary(identifier("i"), BinaryOp::Equal, int_literal_expression(7)),
                break_statement(),
                None,
            ),
        ]),
    );
}

#[test]
fn test_for_loop_with_keyword_as_variable() {
    parser_error_test(
        "
for if in my_list: pass()
",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "if".to_string(),
        },
    );
}

#[test]
fn test_error_multiple_variables_for_range() {
    parser_error_test(
        "for x, y in 1..10",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "a single loop variable for a numeric range".to_string(),
            found: "2 variables".to_string(),
        },
    );
}
