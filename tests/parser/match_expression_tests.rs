// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::lexer::RegexToken;
use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_match_expression_block() {
    parse_test("
match x
    1: print('one')
    2
        print('two')
    default: print('other')
", vec![
        expression_statement(
            match_expression(
                identifier("x"),
                vec![
                    MatchBranch {
                        patterns: vec![MatchPattern::Literal(int_literal(1))],
                        guard: None,
                        body: Box::new(expression_statement(call(identifier("print"), vec![string_literal("one")])))
                    },
                    MatchBranch {
                        patterns: vec![MatchPattern::Literal(int_literal(2))],
                        guard: None,
                        body: Box::new(block(vec![expression_statement(call(identifier("print"), vec![string_literal("two")]))]))
                    },
                    MatchBranch {
                        patterns: vec![MatchPattern::Default],
                        guard: None,
                        body: Box::new(expression_statement(call(identifier("print"), vec![string_literal("other")])))
                    }
                ]
            )
        )
    ]);
}

#[test]
fn test_match_expression_fully_inline() {
    parse_test(
        "match x: 1: 'one', 2: 'two', default: 'other'",
        vec![
            expression_statement(
                match_expression(
                    identifier("x"),
                    vec![
                        MatchBranch {
                            patterns: vec![MatchPattern::Literal(int_literal(1))],
                            guard: None,
                            body: Box::new(expression_statement(string_literal("one")))
                        },
                        MatchBranch {
                            patterns: vec![MatchPattern::Literal(int_literal(2))],
                            guard: None,
                            body: Box::new(expression_statement(string_literal("two")))
                        },
                        MatchBranch {
                            patterns: vec![MatchPattern::Default],
                            guard: None,
                            body: Box::new(expression_statement(string_literal("other")))
                        }
                    ]
                )
            )
        ]
    );
}

#[test]
fn test_match_with_guard() {
    parse_test("
match num
    x if x > 10: 'large'
    x: 'small'
", vec![
        expression_statement(
            match_expression(
                identifier("num"),
                vec![
                    MatchBranch {
                        patterns: vec![MatchPattern::Identifier("x".to_string())],
                        guard: Some(Box::new(binary(identifier("x"), BinaryOp::GreaterThan, int_literal_expression(10)))),
                        body: Box::new(expression_statement(string_literal("large")))
                    },
                    MatchBranch {
                        patterns: vec![MatchPattern::Identifier("x".to_string())],
                        guard: None,
                        body: Box::new(expression_statement(string_literal("small")))
                    }
                ]
            )
        )
    ]);
}

#[test]
fn test_match_with_multiple_patterns() {
    parse_test("
match code
    200 | 201 | 204: 'Success'
    404: 'Not Found'
", vec![
        expression_statement(
            match_expression(
                identifier("code"),
                vec![
                    MatchBranch {
                        patterns: vec![
                            MatchPattern::Literal(int_literal(200)),
                            MatchPattern::Literal(int_literal(201)),
                            MatchPattern::Literal(int_literal(204))
                        ],
                        guard: None,
                        body: Box::new(expression_statement(string_literal("Success")))
                    },
                    MatchBranch {
                        patterns: vec![MatchPattern::Literal(int_literal(404))],
                        guard: None,
                        body: Box::new(expression_statement(string_literal("Not Found")))
                    }
                ]
            )
        )
    ]);
}

#[test]
fn test_match_with_tuple_pattern() {
    parse_test("
match point
    (0, 0): 'origin'
    (x, 0): 'on x-axis'
", vec![
        expression_statement(
            match_expression(
                identifier("point"),
                vec![
                    MatchBranch {
                        patterns: vec![MatchPattern::Tuple(vec![MatchPattern::Literal(int_literal(0)), MatchPattern::Literal(int_literal(0))])],
                        guard: None,
                        body: Box::new(expression_statement(string_literal("origin")))
                    },
                    MatchBranch {
                        patterns: vec![MatchPattern::Tuple(vec![MatchPattern::Identifier("x".to_string()), MatchPattern::Literal(int_literal(0))])],
                        guard: None,
                        body: Box::new(expression_statement(string_literal("on x-axis")))
                    }
                ]
            )
        )
    ]);
}

#[test]
fn test_match_with_regex_pattern() {
    parse_test(r#"
match text
    re"^\d+$": 'digits only'
    re"^[a-z]+$": 'lowercase only'
"#, vec![
        expression_statement(
            match_expression(
                identifier("text"),
                vec![
                    MatchBranch {
                        patterns: vec![
                            MatchPattern::Regex(RegexToken { 
                                body: "^\\d+$".to_string(), 
                                ignore_case: false, 
                                global: false, 
                                multiline: false, 
                                dot_all: false, 
                                unicode: false 
                            })
                        ],
                        guard: None,
                        body: Box::new(expression_statement(string_literal("digits only")))
                    },
                    MatchBranch {
                        patterns: vec![
                            MatchPattern::Regex(RegexToken { 
                                body: "^[a-z]+$".to_string(), 
                                ignore_case: false, 
                                global: false, 
                                multiline: false, 
                                dot_all: false, 
                                unicode: false 
                            })
                        ],
                        guard: None,
                        body: Box::new(expression_statement(string_literal("lowercase only")))
                    }
                ]
            )
        )
    ]);
}

#[test]
fn test_match_missing_body() {
    // TODO: maybe this shouldn't be allowed
parse_test("
match x
    1
", vec![
    expression_statement(
        match_expression(
            identifier("x"),
            vec![
                MatchBranch {
                    patterns: vec![MatchPattern::Literal(int_literal(1))],
                    guard: None,
                    body: Box::new(empty_statement())
                }
            ]
        )
    )
]);
}

#[test]
fn test_error_match_invalid_guard() {
    // Guard must be a valid expression, `1 2` is not.
    parse_error_test("
match x
    y if 1 2: 'invalid'
", SyntaxErrorKind::UnexpectedToken {
    expected: "a colon for an inline body or an indented block for a block body".to_string(),
    found: "int".to_string()
});
}
