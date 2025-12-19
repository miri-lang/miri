// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_parse_empty_program() {
    parser_test("", empty_program());
}

#[test]
fn test_parse_program_with_only_comments_and_whitespace() {
    parser_test(
        "
// This is a comment
    // This is an indented comment

/* Another comment */
",
        empty_program(),
    );
}

#[test]
fn test_parse_simple_expressions() {
    parser_test(
        "
123
'Hello World'
",
        vec![
            expression_statement(int_literal_expression(123)),
            expression_statement(string_literal_expression("Hello World")),
        ],
    );
}
