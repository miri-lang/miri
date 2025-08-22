// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_regex_literal_assignment() {
    parse_test(r#"let pattern = re"^\d+$"im"#, vec![
        variable_statement(vec![
            let_variable(
                "pattern",
                None,
                opt_expr(regex_literal(r"^\d+$", "im"))
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_regex_in_function_call() {
    parse_test(r#"text.matches(re"[a-z]+")"#, vec![
        expression_statement(
            call(
                member(identifier("text"), identifier("matches")),
                vec![regex_literal("[a-z]+", "")]
            )
        )
    ]);
}

#[test]
fn test_regex_with_escaped_quotes() {
    parse_test(r#"let p = re"\"quoted\"""#, vec![
        variable_statement(vec![
            let_variable(
                "p",
                None,
                opt_expr(regex_literal(r#"\"quoted\""#, ""))
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_method_call_on_regex_literal() {
    parse_test(r#"re"abc"g.test("abc")"#, vec![
        expression_statement(
            call(
                member(
                    regex_literal("abc", "g"),
                    identifier("test")
                ),
                vec![string_literal("abc")]
            )
        )
    ]);
}

#[test]
fn test_empty_regex_literal() {
    parse_test(r#"let empty = re""u"#, vec![
        variable_statement(vec![
            let_variable(
                "empty",
                None,
                opt_expr(regex_literal("", "u"))
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_error_regex_prefix_space() {
    // `re "..."` is not a valid regex literal. The lexer sees `re` as an identifier
    // and `"..."` as a string. The parser then fails because an identifier
    // cannot be followed directly by a string in an expression.
    parse_error_test(
        r#"re "abc""#,
        SyntaxErrorKind::UnexpectedToken {
            expected: "newline or end of file".to_string(),
            found: "string".to_string(),
        }
    );
}
