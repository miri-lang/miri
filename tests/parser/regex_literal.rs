// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{parser_error_test, parser_test};
use miri::ast::factory::{
    call, expression_statement, identifier, let_variable, member, regex_literal,
    string_literal_expression, variable_statement,
};
use miri::ast::{opt_expr, MemberVisibility};
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_regex_literal_assignment() {
    parser_test(
        r#"let pattern = re"^\d+$"im"#,
        vec![variable_statement(
            vec![let_variable(
                "pattern",
                None,
                opt_expr(regex_literal(r"^\d+$", "im")),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_regex_in_function_call() {
    parser_test(
        r#"text.matches(re"[a-z]+")"#,
        vec![expression_statement(call(
            member(identifier("text"), identifier("matches")),
            vec![regex_literal("[a-z]+", "")],
        ))],
    );
}

#[test]
fn test_regex_with_escaped_quotes() {
    parser_test(
        r#"let p = re"\"quoted\"""#,
        vec![variable_statement(
            vec![let_variable(
                "p",
                None,
                opt_expr(regex_literal(r#"\"quoted\""#, "")),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_method_call_on_regex_literal() {
    parser_test(
        r#"re"abc"g.test("abc")"#,
        vec![expression_statement(call(
            member(regex_literal("abc", "g"), identifier("test")),
            vec![string_literal_expression("abc")],
        ))],
    );
}

#[test]
fn test_empty_regex_literal() {
    parser_test(
        r#"let empty = re""u"#,
        vec![variable_statement(
            vec![let_variable(
                "empty",
                None,
                opt_expr(regex_literal("", "u")),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_error_regex_prefix_space() {
    // `re "..."` is not a valid regex literal. The lexer sees `re` as an identifier
    // and `"..."` as a string. The parser then fails because an identifier
    // cannot be followed directly by a string in an expression.
    parser_error_test(
        r#"re "abc""#,
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an end of statement".to_string(),
            found: "string".to_string(),
        },
    );
}

#[test]
fn test_regex_with_all_flags() {
    parser_test(
        r#"let p = re"."gimsu"#,
        vec![variable_statement(
            vec![let_variable(
                "p",
                None,
                opt_expr(regex_literal(".", "gimsu")),
            )],
            MemberVisibility::Public,
        )],
    );
}

#[test]
fn test_error_on_unterminated_regex() {
    parser_error_test(r#"let p = re"abc"#, &SyntaxErrorKind::InvalidToken);
}
