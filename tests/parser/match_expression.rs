// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::*;
use miri::lexer::RegexToken;
use miri::syntax_error::SyntaxErrorKind;
use miri::ast_factory::*;
use super::utils::*;


#[test]
fn test_match_expression_block() {
    parser_test("
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
                        patterns: vec![Pattern::Literal(int_literal(1))],
                        guard: None,
                        body: Box::new(expression_statement(call(identifier("print"), vec![string_literal_expression("one")])))
                    },
                    MatchBranch {
                        patterns: vec![Pattern::Literal(int_literal(2))],
                        guard: None,
                        body: Box::new(block(vec![expression_statement(call(identifier("print"), vec![string_literal_expression("two")]))]))
                    },
                    MatchBranch {
                        patterns: vec![Pattern::Default],
                        guard: None,
                        body: Box::new(expression_statement(call(identifier("print"), vec![string_literal_expression("other")])))
                    }
                ]
            )
        )
    ]);
}

#[test]
fn test_match_expression_fully_inline() {
    parser_test(
        "match x: 1: 'one', 2: 'two', default: 'other'",
        vec![
            expression_statement(
                match_expression(
                    identifier("x"),
                    vec![
                        MatchBranch {
                            patterns: vec![Pattern::Literal(int_literal(1))],
                            guard: None,
                            body: Box::new(expression_statement(string_literal_expression("one")))
                        },
                        MatchBranch {
                            patterns: vec![Pattern::Literal(int_literal(2))],
                            guard: None,
                            body: Box::new(expression_statement(string_literal_expression("two")))
                        },
                        MatchBranch {
                            patterns: vec![Pattern::Default],
                            guard: None,
                            body: Box::new(expression_statement(string_literal_expression("other")))
                        }
                    ]
                )
            )
        ]
    );
}

#[test]
fn test_match_with_guard() {
    parser_test("
match num
    x if x > 10: 'large'
    x: 'small'
", vec![
        expression_statement(
            match_expression(
                identifier("num"),
                vec![
                    MatchBranch {
                        patterns: vec![Pattern::Identifier("x".to_string())],
                        guard: Some(Box::new(binary(identifier("x"), BinaryOp::GreaterThan, int_literal_expression(10)))),
                        body: Box::new(expression_statement(string_literal_expression("large")))
                    },
                    MatchBranch {
                        patterns: vec![Pattern::Identifier("x".to_string())],
                        guard: None,
                        body: Box::new(expression_statement(string_literal_expression("small")))
                    }
                ]
            )
        )
    ]);
}

#[test]
fn test_match_with_multiple_patterns() {
    parser_test("
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
                            Pattern::Literal(int_literal(200)),
                            Pattern::Literal(int_literal(201)),
                            Pattern::Literal(int_literal(204))
                        ],
                        guard: None,
                        body: Box::new(expression_statement(string_literal_expression("Success")))
                    },
                    MatchBranch {
                        patterns: vec![Pattern::Literal(int_literal(404))],
                        guard: None,
                        body: Box::new(expression_statement(string_literal_expression("Not Found")))
                    }
                ]
            )
        )
    ]);
}

#[test]
fn test_match_with_tuple_pattern() {
    parser_test("
match point
    (0, 0): 'origin'
    (x, 0): 'on x-axis'
", vec![
        expression_statement(
            match_expression(
                identifier("point"),
                vec![
                    MatchBranch {
                        patterns: vec![Pattern::Tuple(vec![Pattern::Literal(int_literal(0)), Pattern::Literal(int_literal(0))])],
                        guard: None,
                        body: Box::new(expression_statement(string_literal_expression("origin")))
                    },
                    MatchBranch {
                        patterns: vec![Pattern::Tuple(vec![Pattern::Identifier("x".to_string()), Pattern::Literal(int_literal(0))])],
                        guard: None,
                        body: Box::new(expression_statement(string_literal_expression("on x-axis")))
                    }
                ]
            )
        )
    ]);
}

#[test]
fn test_match_with_regex_pattern() {
    parser_test(r#"
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
                            Pattern::Regex(RegexToken { 
                                body: "^\\d+$".to_string(), 
                                ignore_case: false, 
                                global: false, 
                                multiline: false, 
                                dot_all: false, 
                                unicode: false 
                            })
                        ],
                        guard: None,
                        body: Box::new(expression_statement(string_literal_expression("digits only")))
                    },
                    MatchBranch {
                        patterns: vec![
                            Pattern::Regex(RegexToken { 
                                body: "^[a-z]+$".to_string(), 
                                ignore_case: false, 
                                global: false, 
                                multiline: false, 
                                dot_all: false, 
                                unicode: false 
                            })
                        ],
                        guard: None,
                        body: Box::new(expression_statement(string_literal_expression("lowercase only")))
                    }
                ]
            )
        )
    ]);
}

#[test]
fn test_error_match_missing_body() {
parser_error_test("
match x
    1
", &SyntaxErrorKind::UnexpectedToken {
        expected: "a colon for an inline body or an indented block for a block body".to_string(),
        found: "end of expression".to_string()
    });
}

#[test]
fn test_error_match_invalid_guard() {
    // Guard must be a valid expression, `1 2` is not.
    parser_error_test("
match x
    y if 1 2: 'invalid'
", &SyntaxErrorKind::UnexpectedToken {
    expected: "a colon for an inline body or an indented block for a block body".to_string(),
    found: "int".to_string()
});
}

#[test]
fn test_error_duplicate_patterns() {
    parser_error_test("
match x
    1: 'one'
    1: 'duplicate'
", &SyntaxErrorKind::DuplicateMatchPattern
    );
}

#[test]
fn test_error_duplicate_patterns_with_guard() {
    parser_error_test("
match x
    x if x > 0: 'one'
    x if x > 0: 'duplicate'
", &SyntaxErrorKind::DuplicateMatchPattern
    );
}

#[test]
fn test_duplicate_patterns_with_different_guard() {
    parser_test("
match x
    x if x > 0: 'one'
    x if x < 0: 'no duplicate'
", vec![
        expression_statement(
            match_expression(
                identifier("x"),
                vec![
                    MatchBranch {
                        patterns: vec![Pattern::Identifier("x".to_string())],
                        guard: opt_expr(binary(
                            identifier("x"),
                            BinaryOp::GreaterThan,
                            int_literal_expression(0)
                        )),
                        body: Box::new(expression_statement(string_literal_expression("one")))
                    },
                    MatchBranch {
                        patterns: vec![Pattern::Identifier("x".to_string())],
                        guard: opt_expr(binary(
                            identifier("x"),
                            BinaryOp::LessThan,
                            int_literal_expression(0)
                        )),
                        body: Box::new(expression_statement(string_literal_expression("no duplicate")))
                    }
                ]
            )
        )
    ]);
}

#[test]
fn test_match_with_complex_subject() {
    // The subject of a match can be any expression, not just an identifier.
    parser_test("
match get_value() + 1
    1: 'one'
    _: 'other'
", vec![
        expression_statement(
            match_expression(
                binary(
                    call(identifier("get_value"), vec![]),
                    BinaryOp::Add,
                    int_literal_expression(1)
                ),
                vec![
                    MatchBranch {
                        patterns: vec![Pattern::Literal(int_literal(1))],
                        guard: None,
                        body: Box::new(expression_statement(string_literal_expression("one")))
                    },
                    MatchBranch {
                        patterns: vec![Pattern::Identifier("_".to_string())],
                        guard: None,
                        body: Box::new(expression_statement(string_literal_expression("other")))
                    }
                ]
            )
        )
    ]);
}

#[test]
fn test_match_as_return_value() {
    // A match expression can be used as the value in a variable assignment or return.
    parser_test("
let result = match x: 1: 'one', _: 'other'
", vec![
        variable_statement(vec![
            let_variable(
                "result",
                None,
                opt_expr(match_expression(
                    identifier("x"),
                    vec![
                        MatchBranch {
                            patterns: vec![Pattern::Literal(int_literal(1))],
                            guard: None,
                            body: Box::new(expression_statement(string_literal_expression("one")))
                        },
                        MatchBranch {
                            patterns: vec![Pattern::Identifier("_".to_string())],
                            guard: None,
                            body: Box::new(expression_statement(string_literal_expression("other")))
                        }
                    ]
                ))
            )
        ], MemberVisibility::Public)
    ]);
}

#[test]
fn test_nested_match_expression() {
    // The body of a match branch can contain another match expression.
    parser_test("
match a
    1: match b
        2: 'inner'
        _: 'other inner'
    _: 'outer'
", vec![
        expression_statement(
            match_expression(
                identifier("a"),
                vec![
                    MatchBranch {
                        patterns: vec![Pattern::Literal(int_literal(1))],
                        guard: None,
                        body: Box::new(expression_statement(match_expression(
                            identifier("b"),
                            vec![
                                MatchBranch {
                                    patterns: vec![Pattern::Literal(int_literal(2))],
                                    guard: None,
                                    body: Box::new(expression_statement(string_literal_expression("inner")))
                                },
                                MatchBranch {
                                    patterns: vec![Pattern::Identifier("_".to_string())],
                                    guard: None,
                                    body: Box::new(expression_statement(string_literal_expression("other inner")))
                                }
                            ]
                        )))
                    },
                    MatchBranch {
                        patterns: vec![Pattern::Identifier("_".to_string())],
                        guard: None,
                        body: Box::new(expression_statement(string_literal_expression("outer")))
                    }
                ]
            )
        )
    ]);
}

#[test]
fn test_error_on_empty_match() {
    // A match expression must have at least one branch.
    parser_error_test("match x:", &SyntaxErrorKind::MissingMatchBranches);
    parser_error_test("match x\n    \n", &SyntaxErrorKind::MissingMatchBranches);
}
