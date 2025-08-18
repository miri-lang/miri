// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::ast::*;
use miri::lexer::{Lexer};
use miri::parser::Parser;
use miri::syntax_error::{SyntaxError, SyntaxErrorKind};

pub mod ast_builder;
use ast_builder::*;


#[test]
fn test_parse_empty_program() {
    parse_test("", empty_program());
}

#[test]
fn test_parse_program_with_only_comments_and_whitespace() {
    parse_test("
// This is a comment
    // This is an indented comment

/* Another comment */
", empty_program());
}

#[test]
fn test_parse_integer_literal() {
    parse_integer_test("42", int(42));
    parse_integer_test("12345", int(12345));
    parse_integer_test("1_234_567_890", int(1234567890));
    parse_integer_test("9_223_372_036_854_775_807", int(9223372036854775807));

    parse_integer_test("0b1_01_010", int(42));
    parse_integer_test("0xFF", int(255));
    parse_integer_test("0o77", int(63));
    parse_integer_test("0o1234567", int(342391));
}

#[test]
fn test_parse_float_literal() {
    parse_float_test("3.14", float32(3.14));
    parse_float_test("1.797693134862315", float64(1.797693134862315));

    parse_float_test("1_000.0", float32(1_000.0));
    parse_float_test("1_000_000.123456789", float64(1_000_000.123456789));

    parse_float_test("1.0e10", float32(1.0e10));
    parse_float_test("6.67430e-11", float32(6.67430e-11));
}

#[test]
fn test_parse_float_literal_edge_cases() {
    // Precision edge cases
    parse_float_test("3.141592", float32(3.141592)); // fits f32
    parse_float_test("3.1415927", float32(3.1415927)); // still fits
    parse_float_test("3.14159265", float64(3.14159265)); // too long for f32

    // Largest and smallest values
    parse_float_test("3.4028235e38", float32(3.4028235e38)); // max f32
    parse_float_test("1.17549435e-38", float32(1.17549435e-38)); // min normal f32
    parse_float_test("1.7976931348623157e308", float64(1.7976931348623157e308)); // max f64
    parse_float_test("2.2250738585072014e-308", float64(2.2250738585072014e-308)); // min normal f64

    // Zeros
    parse_float_test("0.0", float32(0.0));
    parse_float_test("0.000000", float32(0.0));

    // Underscore formatting
    parse_float_test("123_456.789", float32(123_456.789));
    parse_float_test("1_000_000.1234567", float64(1_000_000.1234567));
    parse_float_test("1_000_000.12345678", float64(1_000_000.12345678)); // too long

    // Scientific notation variants
    parse_float_test("1.0e+10", float32(1.0e+10));
    parse_float_test("1.0E10", float32(1.0E10));
    parse_float_test("1.0000001e10", float32(1.0000001e10_f32)); // precision edge
    parse_float_test("9.999999e+37", float32(9.999999e37)); // edge of f32

    // Negative exponent
    parse_float_test("1.0e-10", float32(1.0e-10));
    parse_float_test("6.02214076e-23", float64(6.02214076e-23)); // Planck constant

    // Extreme edge underflow
    parse_float_test("1e-46", float64(1e-46)); // below f32 subnormal
    parse_float_test("1e-39", float32(1e-39)); // subnormal but fits
}

#[test]
fn test_parse_string_literal() {
    parse_literal_test("'hello single quote'", string("hello single quote"));
    parse_literal_test("\"hello double quote\"", string("hello double quote"));
}

#[test]
fn test_parse_boolean_literal() {
    parse_literal_test("true", boolean(true));
    parse_literal_test("false", boolean(false));
}

#[test]
fn test_parse_symbol_literal() {
    parse_literal_test(":my_fancy_symbol", symbol("my_fancy_symbol"));
}

#[test]
fn test_parse_expressions() {
    parse_test("
123
'Hello World'
", vec![
        expression_statement(
            int_literal(123)
        ),
        expression_statement(
            string_literal("Hello World")
        )
    ]);
}

#[test]
fn test_parse_binary_expression() {
    parse_binary_expression_test(
        "123 + 456",
        int_literal(123),
        BinaryOp::Add,
        int_literal(456)
    );
}

#[test]
fn test_parse_chained_binary_expression() {
    parse_binary_expression_test(
        "123 + 456 - 789",
        binary(
            int_literal(123),
            BinaryOp::Add,
            int_literal(456)
        ),
        BinaryOp::Sub,
        int_literal(789)
    );
}

#[test]
fn test_parse_chained_multiply_expression() {
    parse_test("2 + 2 * 2", vec![
        expression_statement(
            binary(
                int_literal(2),
                BinaryOp::Add,
                binary(int_literal(2), BinaryOp::Mul, int_literal(2))
            )
        )
    ]);
}

#[test]
fn test_parse_bitwise_and_expression() {
    parse_binary_expression_test(
        "1 + 2 & 2",
        binary(
            int_literal(1),
            BinaryOp::Add,
            int_literal(2)
        ),
        BinaryOp::BitwiseAnd,
        int_literal(2)
    );
}

#[test]
fn test_parse_bitwise_or_expression() {
    parse_binary_expression_test(
        "1 + 2 | 2",
        binary(
            int_literal(1),
            BinaryOp::Add,
            int_literal(2)
        ),
        BinaryOp::BitwiseOr,
        int_literal(2)
    );
}

#[test]
fn test_parse_bitwise_xor_expression() {
    parse_binary_expression_test(
        "1 + 2 ^ 2",
        binary(
            int_literal(1),
            BinaryOp::Add,
            int_literal(2)
        ),
        BinaryOp::BitwiseXor,
        int_literal(2)
    );
}


#[test]
fn test_parse_multiply_with_parentheses_expression() {
    parse_binary_expression_test(
        "(2 + 2) * 2",
        binary(
            int_literal(2),
            BinaryOp::Add,
            int_literal(2)
        ),
        BinaryOp::Mul,
        int_literal(2)
    );
}

#[test]
fn test_parse_simple_parentheses_expression() {
    parse_test("
(123)
", vec![
        expression_statement(
            int_literal(123)
        )
    ]);
}

#[test]
fn test_parse_consecutive_operators() {
    // Two binary operators in a row is invalid.
    parse_error_test(
        "5 + * 2", 
        SyntaxErrorKind::UnexpectedToken { 
            expected: "literal, parenthesized expression or identifier".into(), 
            found: "*".into() 
        }
    );
}

#[test]
fn test_parse_mismatched_parentheses() {
    // Mismatched brackets should be a syntax error.
    parse_error_test(
        "(5 + 2]", 
        SyntaxErrorKind::UnexpectedToken { 
            expected: ")".into(),
            found: "]".into() 
        }
    );
}

#[test]
fn test_parse_incomplete_expression() {
    // The parser should error on an incomplete binary expression.
    parse_error_test(
        "5 +", 
        SyntaxErrorKind::UnexpectedEOF
    );
}

#[test]
fn test_parse_invalid_assignment_target() {
    // The left-hand side of an assignment must be a valid target (e.g., identifier).
    // An expression like `x + 1` is not a valid target.
    parse_error_test("x + 1 = 10", SyntaxErrorKind::InvalidLeftHandSideExpression);
}

#[test]
fn test_parse_invalid_variable_declaration() {
    // A literal cannot be a variable name.
    parse_error_test("let 123 = 456", SyntaxErrorKind::UnexpectedToken {
        expected: "identifier".into(),
        found: "int".into(),
    });
}

#[test]
fn test_parse_assignment_expression() {
    parse_assignment_expression_test(
        "x = 123", 
        lhs_identifier("x".into()), 
        AssignmentOp::Assign, 
        int_literal(123)
    );
}


#[test]
fn test_parse_chained_assignment_expression() {
    parse_assignment_expression_test(
        "x = y = 123", 
        lhs_identifier("x".into()), 
        AssignmentOp::Assign, 
        assign(
            lhs_identifier("y".into()),
            AssignmentOp::Assign,
            int_literal(123)
        )
    );
}

#[test]
fn test_parse_increment_assignment_expression() {
    parse_assignment_expression_test(
        "x += 100", 
        lhs_identifier("x".into()), 
        AssignmentOp::AssignAdd,
        int_literal(100)
    );
}

#[test]
fn test_parse_decrement_assignment_expression() {
    parse_assignment_expression_test(
        "x -= 200", 
        lhs_identifier("x".into()), 
        AssignmentOp::AssignSub,
        int_literal(200)
    );
}

#[test]
fn test_parse_multiplication_assignment_expression() {
    parse_assignment_expression_test(
        "x *= 10", 
        lhs_identifier("x".into()), 
        AssignmentOp::AssignMul,
        int_literal(10)
    );
}

#[test]
fn test_parse_division_assignment_expression() {
    parse_assignment_expression_test(
        "x /= 10", 
        lhs_identifier("x".into()), 
        AssignmentOp::AssignDiv,
        int_literal(10)
    );
}

#[test]
fn test_parse_modulo_assignment_expression() {
    parse_assignment_expression_test(
        "x %= 10", 
        lhs_identifier("x".into()), 
        AssignmentOp::AssignMod,
        int_literal(10)
    );
}

#[test]
fn test_parse_increment_chained_assignment_expression() {
    parse_assignment_expression_test(
        "x = y = z += 100",
        lhs_identifier("x".into()),
        AssignmentOp::Assign,
        assign(
            lhs_identifier("y".into()),
            AssignmentOp::Assign,
            assign(
                lhs_identifier("z".into()),
                AssignmentOp::AssignAdd,
                int_literal(100)
            )
        )
    );
}

#[test]
fn test_parse_variable_declaration() {
    parse_variable_declaration_test(
        "let x = 5",
        vec![
            let_variable("x", None, opt_expr(int_literal(5)))
        ]
    );
}

#[test]
fn test_parse_typed_variable_declaration() {
    parse_variable_declaration_test(
        "let x int = 5",
        vec![
            let_variable("x", opt_expr(typ(Type::Int)), opt_expr(int_literal(5)))
        ]
    );
}

#[test]
fn test_parse_typed_variable_declaration_no_initializer() {
    parse_variable_declaration_test(
        "let x float",
        vec![
            let_variable("x", opt_expr(typ(Type::Float)), None)
        ]
    );
}

#[test]
fn test_parse_variable_declaration_no_initializer() {
    parse_variable_declaration_test(
        "let x",
        vec![
            let_variable("x", None, None)
        ]
    );
}

#[test]
fn test_parse_mutable_variable_declaration() {
    parse_variable_declaration_test(
        "var text = \"Hello, World!\"",
        vec![
            var("text", None, opt_expr(string_literal("Hello, World!")))
        ]
    );
}

#[test]
fn test_parse_multiple_variable_declaration_no_initializer() {
    parse_variable_declaration_test(
        "let x, y, z",
        vec![
            let_variable("x", None, None),
            let_variable("y", None, None),
            let_variable("z", None, None)
        ]
    );
}

#[test]
fn test_parse_multiple_variable_declaration_mixed_initializer() {
    parse_variable_declaration_test(
        "let x, y = 10, z",
        vec![
            let_variable("x", None, None),
            let_variable("y", None, opt_expr(int_literal(10))),
            let_variable("z", None, None)
        ]
    );
}

#[test]
fn test_parse_variable_declaration_and_assignment() {
    parse_test("
var bar = 100
let foo = bar = 200
",
        vec![
            variable_statement(
                vec![var("bar", None, opt_expr(int_literal(100)))]
            ),
            variable_statement(
                vec![let_variable("foo", None, opt_expr(assign(lhs_identifier("bar"), AssignmentOp::Assign, int_literal(200))))]
            )
        ]
    );
}

#[test]
fn test_parse_if_expression() {
    parse_if_test("
if x
    x = 10
else
    x = 20
",
    identifier("x".into()),
    block(vec![
        expression_statement(
            assign(
                lhs_identifier("x"),
                AssignmentOp::Assign,
                int_literal(10)
            )
        )
    ]),
    Some(
        block(vec![
            expression_statement(
                assign(
                    lhs_identifier("x"),
                    AssignmentOp::Assign,
                    int_literal(20)
                )
            )
        ]),
    ));
}

#[test]
fn test_parse_if_expression_with_condition() {
    parse_if_test("
if x > 5
    x = 10
else
    x = 20
",
    binary(identifier("x"), BinaryOp::GreaterThan, int_literal(5)),
    block(vec![
        expression_statement(assign(lhs_identifier("x"), AssignmentOp::Assign, int_literal(10)))
    ]),
        Some(
            block(vec![
                expression_statement(assign(lhs_identifier("x"), AssignmentOp::Assign, int_literal(20)))
            ])
        )
    );
}

#[test]
fn test_parse_if_block_else_inline() {
    parse_if_test("
if x
    x = 10
else: x = 20
",
    identifier("x".into()),
    block(vec![
        expression_statement(
            assign(
                lhs_identifier("x"),
                AssignmentOp::Assign,
                int_literal(10)
            )
        )
    ]),
    Some(
        expression_statement(
            assign(
                lhs_identifier("x"),
                AssignmentOp::Assign,
                int_literal(20)
            )
        )
    ));
}

#[test]
fn test_parse_if_inline_else_block() {
    parse_if_test("
if x: x = 10
else
    x = 20
",
    identifier("x".into()),
    expression_statement(
        assign(
            lhs_identifier("x"),
            AssignmentOp::Assign,
            int_literal(10)
        )
    ),
    Some(
        block(vec![
            expression_statement(
                assign(
                    lhs_identifier("x".into()),
                    AssignmentOp::Assign,
                    int_literal(20)
                )
            )
        ]),
    ));
}

#[test]
fn test_parse_if_expression_no_else() {
    parse_if_test("
if x
    x = 10
",
    identifier("x".into()),
    Statement::Block(vec![
        expression_statement(
            assign(
                lhs_identifier("x".into()),
                AssignmentOp::Assign,
                int_literal(10)
            )
        )
    ]),
    None
    );
}

#[test]
fn test_parse_if_expression_nested() {
    parse_if_expression_test("
if x
    if y
        x = 10
    else
        x = 20
else
    if z
        if w
            x = 30
    else
        x = 40
",
    identifier("x".into()),
    block(vec![
        if_statement(
            identifier("y".into()),
            block(vec![
                expression_statement(
                    assign(
                        lhs_identifier("x".into()),
                        AssignmentOp::Assign,
                        int_literal(10)
                    )
                )
            ]),
            Some(
                block(vec![
                    expression_statement(
                        assign(
                            lhs_identifier("x".into()),
                            AssignmentOp::Assign,
                            int_literal(20)
                        )
                    )
                ])
            )
        )
    ]),
    Some(
        block(vec![
            if_statement(
                identifier("z".into()),
                block(vec![
                    if_statement(
                        identifier("w".into()),
                        block(vec![
                            expression_statement(
                                assign(
                                    lhs_identifier("x".into()),
                                    AssignmentOp::Assign,
                                    int_literal(30)
                                )
                            )
                        ]),
                        None
                    )
                ]),
                Some(
                    block(vec![
                        expression_statement(
                            assign(
                                lhs_identifier("x".into()),
                                AssignmentOp::Assign,
                                int_literal(40)
                            )
                        )
                    ])
                )
            )
        ])
    ), 
    IfStatementType::If
    );
}

#[test]
fn test_parse_if_expression_inline() {
    parse_if_test("
if x: x = 10 else: x = 20
",
    identifier("x".into()),
    expression_statement(
        assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal(10)
        )
    ),
    Some(
            expression_statement(
                assign(
                    lhs_identifier("x".into()),
                    AssignmentOp::Assign,
                    int_literal(20)
                )
            )
        )
    );
}

#[test]
fn test_parse_if_mixed_inline() {
    parse_if_test("
if x: x = 10
else: x = 20
",
    identifier("x".into()),
    expression_statement(
        assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal(10) 
        )
    ),
    Some(
            expression_statement(
                assign(
                    lhs_identifier("x".into()),
                    AssignmentOp::Assign,
                    int_literal(20)
                )
            )
        )
    );
}

#[test]
fn test_parse_if_expression_inline_nested() {
    parse_if_expression_test("
// This is crazy, but should work
if x: if y: x = 10 else: if z: x = 20 else: x = 30
",
    identifier("x".into()),
    if_statement(
    identifier("y".into()),
        expression_statement(
            assign(
                lhs_identifier("x".into()),
                AssignmentOp::Assign,
                int_literal(10)
            )
        ),
        Some(
            if_statement(
                identifier("z".into()),
                expression_statement(
                    assign(
                        lhs_identifier("x".into()),
                        AssignmentOp::Assign,
                        int_literal(20)
                    )
                ),
                Some(
                    expression_statement(
                        assign(
                            lhs_identifier("x".into()),
                            AssignmentOp::Assign,
                            int_literal(30)
                        )
                    )
                ),
            )
        ),
    ),
    None,
    IfStatementType::If
    );
}

#[test]
fn test_parse_if_expression_inline_no_else() {
    parse_if_test("
if x: x = 10
",
    identifier("x".into()),
    expression_statement(
        assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal(10)
        )
    ),
    None
    );
}

#[test]
fn test_parse_if_expression_precedence() {
    parse_if_test("
if x + 10 <= 20: x = 10
",
    binary(
        binary(
            identifier("x".into()),
            BinaryOp::Add,
            int_literal(10)
        ),
        BinaryOp::LessThanEqual,
        int_literal(20)
    ),
    expression_statement(
        assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal(10)
        )
    ),
    None
    );
}

#[test]
fn test_parse_if_else_if_chain() {
    parse_if_expression_test("
if x > 10
    y = 1
else if x > 5
    y = 2
else
    y = 3
",
    binary(
        identifier("x".into()),
        BinaryOp::GreaterThan,
        int_literal(10)
    ),
    block(vec![
        expression_statement(assign(
            lhs_identifier("y".into()),
            AssignmentOp::Assign,
            int_literal(1)
        ))
    ]),
    Some(
        if_statement(
            binary(
                identifier("x".into()),
                BinaryOp::GreaterThan,
                int_literal(5)
            ),
            block(vec![
                expression_statement(
                    assign(
                        lhs_identifier("y".into()),
                        AssignmentOp::Assign,
                        int_literal(2)
                    )
                )
            ]),
            Some(block(vec![
                expression_statement(
                    assign(
                        lhs_identifier("y".into()),
                        AssignmentOp::Assign,
                        int_literal(3)
                    )
                )
            ])),
        )
    ),
    IfStatementType::If
    );
}

#[test]
fn test_parse_if_with_variable_declaration() {
    parse_if_test("
if x
    let y = 10
else
    var z = 20
",
    identifier("x".into()),
    block(vec![
        variable_statement(vec![
            let_variable("y".into(), None, opt_expr(int_literal(10)))
        ])
    ]),
    Some(block(vec![
        variable_statement(vec![
            var("z".into(), None, opt_expr(int_literal(20))),
        ])
    ]))
    );
}

#[test]
fn test_parse_if_with_complex_logical_condition() {
    parse_if_test("
if (x > 10 and y < 5) or z == 1
    x = 1
",
    logical(
        logical(
            binary(
                identifier("x".into()),
                BinaryOp::GreaterThan,
                int_literal(10)
            ),
            BinaryOp::And,
            binary(
                identifier("y".into()),
                BinaryOp::LessThan,
                int_literal(5)
            )
        ),
        BinaryOp::Or,
        binary(
            identifier("z".into()),
            BinaryOp::Equal,
            int_literal(1)
        )
    ),
    block(vec![
        expression_statement(
            assign(
                lhs_identifier("x".into()),
                AssignmentOp::Assign,
                int_literal(1)
            )
        )
    ]),
    None
    );
}

#[test]
fn test_parse_if_with_empty_block() {
    parse_if_test("
if x
    // empty then
else
    x = 1
",
    identifier("x".into()),
    empty_statement(),
    Some(block(vec![
        expression_statement(assign(
            lhs_identifier("x".into()),
            AssignmentOp::Assign,
            int_literal(1)
        ))
    ]))
    );
}

#[test]
fn test_parse_if_with_empty_block_no_else() {
    parse_if_test("
if x
    // TODO
",
    identifier("x".into()),
    empty_statement(),
    None
    );
}

#[test]
fn test_comment_in_empty_block() {
    // An indented block containing only a comment should parse as an empty block.
    parse_test("
if x
    // This block is empty
let y = 1
", vec![
        if_statement(
            identifier("x"),
            empty_statement(),
            None
        ),
        variable_statement(vec![
            let_variable("y", None, opt_expr(int_literal(1)))
        ])
    ]);
}

#[test]
fn test_error_if_statement_as_condition() {
    // An `if` statement is not an expression and cannot be a condition.
    // The parser should expect an expression and fail on the block/inline body.
    parse_error_test(
        "if if x: 1",
        SyntaxErrorKind::UnexpectedToken {
            expected: "literal, parenthesized expression or identifier".to_string(),
            found: "if".to_string(),
        }
    );
}

#[test]
fn test_parse_if_nested_empty() {
    parse_if_expression_test("
if x
    if y
        // TODO
",
    identifier("x".into()),
    block(vec![
        if_statement(
            identifier("y".into()),
            empty_statement(),
            None
        )
    ]),
    None,
    IfStatementType::If
    );
}

#[test]
fn test_parse_if_with_empty_else_block() {
    parse_if_test("
if x
    x = 1
else
    // empty else
",
    identifier("x".into()),
    block(vec![
        expression_statement(
            assign(
                lhs_identifier("x".into()),
                AssignmentOp::Assign,
                int_literal(1)
            )
        )
    ]),
    None
    );
}

#[test]
fn test_parse_if_with_empty_else_block_with_followup() {
    parse_test("
if x
    x = 1
else
    // empty else
x = 2
",
        vec![
            if_statement(
                identifier("x".into()),
                block(vec![
                    expression_statement(
                        assign(
                            lhs_identifier("x".into()),
                            AssignmentOp::Assign,
                            int_literal(1)
                        )
                    )
                ]),
                None,
            ),
            expression_statement(
                assign(
                    lhs_identifier("x".into()),
                    AssignmentOp::Assign,
                    int_literal(2)
                )
            )
        ]
    );
}

#[test]
fn test_parse_if_with_empty_inline_else_block_with_followup() {
    parse_test("
if x
    x = 1
else: // empty else
x = 2
",
        vec![
            if_statement(
                identifier("x".into()),
                block(vec![
                    expression_statement(
                        assign(
                            lhs_identifier("x".into()),
                            AssignmentOp::Assign,
                            int_literal(1)
                        )
                    )
                ]),
                None
            ),
            expression_statement(
                assign(
                    lhs_identifier("x".into()),
                    AssignmentOp::Assign,
                    int_literal(2)
                )
            )
        ]
    );
}

#[test]
fn test_error_dangling_else() {
    // An `else` without a preceding `if` is a syntax error.
    parse_error_test(
        "else: print('error')",
        SyntaxErrorKind::UnexpectedToken {
            expected: "literal, parenthesized expression or identifier".to_string(), // Or a more specific expectation
            found: "else".to_string(),
        }
    );
}

#[test]
fn test_equality_expression() {
    parse_test("
x > 10 == false
",
        vec![
            expression_statement(
                binary(
                    binary(
                        identifier("x".into()),
                        BinaryOp::GreaterThan,
                        int_literal(10)
                    ),
                    BinaryOp::Equal,
                    boolean_literal(false)
                )
            )
        ]
    );
}

#[test]
fn test_equality_expression_not_equal() {
    parse_test("
x >= 8 != true
",
        vec![
            expression_statement(
                binary(
                    binary(
                        identifier("x".into()),
                        BinaryOp::GreaterThanEqual,
                        int_literal(8)
                    ),
                    BinaryOp::NotEqual,
                    boolean_literal(true)
                )
            )
        ]
    );
}

#[test]
fn test_logical_expression() {
    parse_test("
x > 10 and y <= 8
",
        vec![
            expression_statement(
                logical(
                    binary(
                        identifier("x".into()),
                        BinaryOp::GreaterThan,
                        int_literal(10)
                    ),
                    BinaryOp::And,
                    binary(
                        identifier("y".into()),
                        BinaryOp::LessThanEqual,
                        int_literal(8)
                    )
                )
            )
        ]
    );
}

#[test]
fn test_logical_expression_and_precedence() {
    parse_test("
x > 1 and y <= 2 or y == 10
",
        vec![
            expression_statement(
                logical(
                    logical(
                        binary(
                            identifier("x".into()),
                            BinaryOp::GreaterThan,
                            int_literal(1)
                        ),
                        BinaryOp::And,
                        binary(
                            identifier("y".into()),
                            BinaryOp::LessThanEqual,
                            int_literal(2)
                        )
                    ),
                    BinaryOp::Or,
                    binary(
                        identifier("y".into()),
                        BinaryOp::Equal,
                        int_literal(10)
                    )
                )
            )
        ]
    );
}

#[test]
fn test_unary_expression_negate() {
    parse_unary_expression_test("-x", UnaryOp::Negate, identifier("x".into()));
}

#[test]
fn test_unary_expression_plus() {
    parse_unary_expression_test("+x", UnaryOp::Plus, identifier("x".into()));
}

#[test]
fn test_unary_expression_not() {
    parse_unary_expression_test("not x", UnaryOp::Not, identifier("x".into()));
}

#[test]
fn test_unary_expression_bitwise_not() {
    parse_unary_expression_test("~x", UnaryOp::BitwiseNot, identifier("x".into()));
}

#[test]
fn test_unary_expression_increment() {
    parse_unary_expression_test("++x", UnaryOp::Increment, identifier("x".into()));
}

#[test]
fn test_unary_expression_decrement() {
    parse_unary_expression_test("--x", UnaryOp::Decrement, identifier("x".into()));
}

#[test]
fn test_unary_expression_precedence() {
    parse_test("-x * -2", vec![
        expression_statement(
            binary(
                unary(UnaryOp::Negate, identifier("x".into())),
                BinaryOp::Mul,
                unary(UnaryOp::Negate, int_literal(2))
            )
        )
    ]);
}

#[test]
fn test_while_loop() {
    parse_while_test("
while x > 0
    x -= 1
",
    binary(
        identifier("x".into()),
        BinaryOp::GreaterThan,
        int_literal(0)
    ),
    block(vec![
        expression_statement(
            assign(
                lhs_identifier("x"),
                AssignmentOp::AssignSub,
                int_literal(1)
            )
        )
    ])
    );
}

#[test]
fn test_while_loop_empty() {
    parse_while_test("
while x > 0
    // TODO
",
    binary(
        identifier("x".into()),
        BinaryOp::GreaterThan,
        int_literal(0)
    ),
    empty_statement()
    );
}

#[test]
fn test_while_loop_nested() {
    parse_while_expression_test("
while x > 0
    while y < 5
        y += 1
",
    binary(
        identifier("x".into()),
        BinaryOp::GreaterThan,
        int_literal(0)
    ),
    block(vec![
        while_statement(
            binary(
                identifier("y".into()),
                BinaryOp::LessThan,
                int_literal(5)
            ),
            block(vec![
                expression_statement(
                    assign(
                        lhs_identifier("y"),
                        AssignmentOp::AssignAdd,
                        int_literal(1)
                    )
                )
            ])
        )
    ]),
    WhileStatementType::While
    );
}


#[test]
fn test_while_loop_nested_empty() {
    parse_while_expression_test("
while x > 0
    while y < 5
        // TODO
",
    binary(
        identifier("x".into()),
        BinaryOp::GreaterThan,
        int_literal(0)
    ),
    block(vec![
        while_statement(
            binary(
                identifier("y".into()),
                BinaryOp::LessThan,
                int_literal(5)
            ),
            empty_statement()
        )
    ]),
    WhileStatementType::While
    );
}

#[test]
fn test_while_loop_inline() {
    parse_while_test("
while x > 0: x -= 1
",
    binary(
        identifier("x".into()),
        BinaryOp::GreaterThan,
        int_literal(0)
    ),
    expression_statement(
        assign(
            lhs_identifier("x"),
            AssignmentOp::AssignSub,
            int_literal(1)
            )
        )
    );
}

#[test]
fn test_while_loop_containing_if_statement() {
    parse_while_test("
while x < 10
    if x % 2 == 0
        x += 1
    else
        x += 2
",
    binary(
        identifier("x".into()),
        BinaryOp::LessThan,
        int_literal(10)
    ),
    block(vec![
        if_statement(
            binary(
                binary(
                    identifier("x".into()),
                    BinaryOp::Mod,
                    int_literal(2)
                ),
                BinaryOp::Equal,
                int_literal(0)
            ),
            block(vec![
                expression_statement(
                    assign(
                        lhs_identifier("x"),
                        AssignmentOp::AssignAdd,
                        int_literal(1)
                    )
                )
            ]),
            Some(
                block(vec![
                    expression_statement(
                        assign(
                            lhs_identifier("x"),
                            AssignmentOp::AssignAdd,
                            int_literal(2)
                        )
                    )
                ])
            )
        )
    ])
    );
}

#[test]
fn test_parse_conditional_expression() {
    parse_test("
let x = 10 if y > 5 else 20
",
    vec![
        variable_statement(vec![
            let_variable(
                "x".into(),
                None,
                opt_expr(
                    if_conditional(
                        int_literal(10),
                        binary(
                            identifier("y".into()),
                            BinaryOp::GreaterThan,
                            int_literal(5)
                        ),
                        Some(int_literal(20)),
                    )
                )
            )
        ])
    ]
    )
}

#[test]
fn test_parse_conditional_expression_no_else() {
    parse_test("
var x = 100 if y % 2 == 0
",
    vec![
        variable_statement(vec![
            var(
                "x".into(),
                None,
                opt_expr(
                    if_conditional(
                        int_literal(100),
                        binary(
                            binary(
                                identifier("y".into()),
                                BinaryOp::Mod,
                                int_literal(2)
                            ),
                            BinaryOp::Equal,
                            int_literal(0)
                        ),
                        None,
                    )
                ),
            )
        ])
    ]);
}

#[test]
fn test_parse_conditional_expression_with_unless() {
    parse_test("
var x = 1 unless y
",
    vec![
        variable_statement(vec![
            var(
                "x".into(),
                None,
                opt_expr(
                    unless_conditional(
                        int_literal(1),
                        identifier("y".into()),
                        None
                    )
                )
            )
        ])
    ])
}

#[test]
fn test_conditional_expression_as_if_condition() {
    // Using a ternary-style if as the condition for a statement-style if.
    parse_if_expression_test("
if a if b else c
    x = 1
",
        if_conditional(
            identifier("a".into()),
            identifier("b".into()),
            Some(identifier("c".into())),
        ),
        block(vec![
            expression_statement(
                assign(
                    lhs_identifier("x"),
                    AssignmentOp::Assign,
                    int_literal(1)
                )
            )
        ]),
        None,
        IfStatementType::If
    );
}

#[test]
fn test_precedence_of_bitwise_and_equality() {
    // Equality (==) has lower precedence than bitwise AND (&).
    // This should parse as `(x & 10) == 10`.
    parse_test("x & 10 == 10", vec![
        expression_statement(
            binary(
                binary(
                    identifier("x".into()),
                    BinaryOp::BitwiseAnd,
                    int_literal(10)
                ),
                BinaryOp::Equal,
                int_literal(10)
            )
        )
    ]);
}

#[test]
fn test_precedence_of_logical_and_or() {
    // `and` has higher precedence than `or`.
    // This should parse as `(true and false) or true`.
    parse_test("true and false or true", vec![
        expression_statement(
            logical(
                logical(
                    boolean_literal(true),
                    BinaryOp::And,
                    boolean_literal(false)
                ),
                BinaryOp::Or,
                boolean_literal(true)
            )
        )
    ]);
}

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
        int_literal(1),
        opt_expr(int_literal(5)),
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
        int_literal(1),
        opt_expr(int_literal(5)),
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
        range(int_literal(1), opt_expr(int_literal(3)), RangeExpressionType::Exclusive),
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
        range(int_literal(1), opt_expr(int_literal(3)), RangeExpressionType::Exclusive),
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
        range(int_literal(1), opt_expr(int_literal(10)), RangeExpressionType::Inclusive),
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
fn test_error_for_loop_with_complex_iterable() {
    // The current parser only supports simple identifiers or literals in ranges.
    parse_error_test(
        "for x in (get_items())",
        SyntaxErrorKind::UnexpectedToken {
            expected: "an identifier, a string or a number".to_string(),
            found: "(".to_string(),
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

#[test]
fn test_forever_loop() {
    parse_test("
forever
    x
", vec![
        forever_statement(
            block(vec![
                expression_statement(identifier("x".into()))
            ])
        )
    ]);
}

#[test]
fn test_forever_loop_with_comment() {
    parse_test("
forever // This is an infinite loop
    x
", vec![
        forever_statement(
            block(vec![
                expression_statement(identifier("x".into()))
            ])
        )
    ]);
}

#[test]
fn test_forever_loop_with_empty_body_and_comment() {
    parse_test("
forever
    // This is an infinite loop
", vec![
        forever_statement(
            empty_statement()
        )
    ]);
}

#[test]
fn test_forever_loop_nested_with_empty_body_and_comment() {
    parse_test("
forever
    forever
        // This is an infinite loop
", vec![
        forever_statement(
            block(vec![
                forever_statement(
                    empty_statement()
                )
            ])
        )
    ]);
}

#[test]
fn test_forever_loop_inline() {
    parse_test("
forever: x
", vec![
        forever_statement(
            expression_statement(identifier("x".into()))
        )
    ]);
}

#[test]
fn test_forever_loop_inline_with_empty_body_and_comment() {
    parse_test("
forever: // This is an infinite loop
", vec![
        forever_statement(
            empty_statement()
        )
    ]);
}

#[test]
fn test_forever_loop_inline_nested_with_empty_body_and_comment() {
    parse_test("
forever: forever: // This is an infinite loop
", vec![
        forever_statement(
            forever_statement(
                empty_statement()
            )
        )
    ]);
}

#[test]
fn test_function_declaration() {
    parse_test("
def square(x int)
    x * x
", vec![
        def("square".into(),
            None,
            vec![
                parameter("x".into(), opt_expr(typ(Type::Int)), None)
            ],
            None,
            block(vec![
                expression_statement(
                    binary(
                        identifier("x".into()),
                        BinaryOp::Mul,
                        identifier("x".into())
                    )
                )
            ])
        )
    ]);
}

#[test]
fn test_function_declaration_with_guard() {
    parse_test("
def square(x int > 0)
    x * x
", vec![
        def("square".into(),
            None,
            vec![
                parameter("x".into(), opt_expr(typ(Type::Int)), opt_expr(guard(GuardOp::GreaterThan, int_literal(0))))
            ],
            None,
            block(vec![
                expression_statement(
                    binary(
                        identifier("x".into()),
                        BinaryOp::Mul,
                        identifier("x".into())
                    )
                )
            ])
        )
    ]);
}

#[test]
fn test_inline_function_declaration_with_guard() {
    parse_test("
def square(x int > 0) int: x * x
", vec![
        def("square".into(), 
            None,
            vec![
                parameter("x".into(), opt_expr(typ(Type::Int)), opt_expr(guard(GuardOp::GreaterThan, int_literal(0)))),
            ],
            opt_expr(typ(Type::Int)),
            expression_statement(
                binary(
                    identifier("x".into()),
                    BinaryOp::Mul,
                    identifier("x".into())
                )
            )
        )
    ]);
}

#[test]
fn test_function_no_parameters() {
    parse_test("
def get_answer() int: 42
", vec![
        def(
            "get_answer".into(),
            None,
            vec![], // No parameters
            opt_expr(typ(Type::Int)),
            expression_statement(int_literal(42))
        )
    ]);
}

#[test]
fn test_function_multiple_parameters() {
    parse_test("
def add(a int, b int)
    return a + b
", vec![
        def(
            "add".into(),
            None,
            vec![
                parameter("a".into(), opt_expr(typ(Type::Int)), None),
                parameter("b".into(), opt_expr(typ(Type::Int)), None),
            ],
            None,
            block(vec![
                return_statement(
                    opt_expr(binary(identifier("a".into()), BinaryOp::Add, identifier("b".into())))
                )
            ])
        )
    ]);
}

#[test]
fn test_function_untyped_parameter() {
    parse_test("
def process(data)
    // do something
", vec![
        def(
            "process".into(),
            None,
            vec![
                parameter("data".into(), None, None)
            ],
            None,
            empty_statement()
        )
    ]);
}

#[test]
fn test_function_empty_body_block() {
    parse_test("
def no_op()
    // This function does nothing
", vec![
        def(
            "no_op".into(),
            None,
            vec![],
            None,
            empty_statement()
        )
    ]);
}

#[test]
fn test_function_empty_body_inline() {
    parse_test("
def no_op_inline(): // This function also does nothing
", vec![
        def(
            "no_op_inline".into(),
            None,
            vec![],
            None,
            empty_statement()
        )
    ]);
}

#[test]
fn test_error_function_missing_name() {
    parse_error_test(
        "def () int: 42",
        SyntaxErrorKind::UnexpectedToken {
            expected: "function name".to_string(),
            found: "(".to_string(),
        }
    );
}

#[test]
fn test_error_function_missing_parens() {
    parse_error_test(
        "def my_func int: 42",
        SyntaxErrorKind::UnexpectedToken {
            expected: "(".to_string(),
            found: "identifier".to_string(),
        }
    );
}

#[test]
fn test_error_function_invalid_parameter() {
    parse_error_test(
        "def my_func(123)",
        SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "int".to_string(),
        }
    );
}

#[test]
fn test_error_function_trailing_comma_in_params() {
    parse_error_test(
        "def my_func(a, )",
        SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: ")".to_string(),
        }
    );
}

#[test]
fn test_function_with_single_generic_type() {
    parse_test("
def my_func<T>()
    // body
", vec![
        def(
            "my_func".into(),
            Some(vec![generic_type("T", None)]),
            vec![],
            None,
            empty_statement()
        )
    ]);
}

#[test]
fn test_function_with_multiple_generic_types() {
    parse_test("
def my_func<K, V>()
    // body
", vec![
        def(
            "my_func".into(),
            Some(vec![
                generic_type("K", None),
                generic_type("V", None)
            ]),
            vec![],
            None,
            empty_statement()
        )
    ]);
}

#[test]
fn test_function_with_constrained_generic_type() {
    parse_test("
def my_func<T extends SomeClass>()
    // body
", vec![
        def(
            "my_func".into(),
            Some(vec![
                generic_type("T", opt_expr(typ(Type::Custom("SomeClass".into(), None))))
            ]),
            vec![],
            None,
            empty_statement()
        )
    ]);
}

#[test]
fn test_function_with_mixed_generic_types() {
    parse_test("
def my_func<K, V extends SomeTrait>()
    // body
", vec![
        def(
            "my_func".into(),
            Some(vec![
                generic_type("K", None),
                generic_type("V", opt_expr(typ(Type::Custom("SomeTrait".into(), None))))
            ]),
            vec![],
            None,
            empty_statement()
        )
    ]);
}

#[test]
fn test_function_using_generic_types() {
    parse_test("
def process<T>(data T) T: data
", vec![
        def(
            "process".into(),
            Some(vec![generic_type("T", None)]),
            vec![
                parameter("data".into(), opt_expr(typ(Type::Custom("T".into(), None))), None)
            ],
            opt_expr(typ(Type::Custom("T".into(), None))),
            expression_statement(identifier("data"))
        )
    ]);
}

#[test]
fn test_error_function_unclosed_generics() {
    parse_error_test(
        "def my_func<T",
        SyntaxErrorKind::UnexpectedEOF
    );
}

#[test]
fn test_error_function_empty_generics() {
    parse_error_test(
        "def my_func<>()",
        SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: ">".to_string(),
        }
    );
}

#[test]
fn test_error_function_trailing_comma_in_generics() {
    parse_error_test(
        "def my_func<T,>()",
        SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: ">".to_string(),
        }
    );
}

#[test]
fn test_return_statement() {
    parse_test("
return 42
", vec![
        return_statement(opt_expr(int_literal(42)))
    ]);
}

#[test]
fn test_return_statement_with_expression() {
    parse_test("
return 42 + x
", vec![
        return_statement(opt_expr(binary(int_literal(42), BinaryOp::Add, identifier("x".into()))))
    ]);
}

#[test]
fn test_empty_return_statement() {
    parse_test("
return
", vec![
        return_statement(None)
    ]);
}

#[test]
fn test_member_expression() {
    parse_test("
obj.prop
", vec![
        expression_statement(
            member(identifier("obj".into()), identifier("prop").into())
        )
    ]);
}

#[test]
fn test_assign_to_member_expression() {
    parse_test("
obj.prop = 1
", vec![
        expression_statement(
            assign(
                lhs_member(identifier("obj".into()), identifier("prop").into()),
                AssignmentOp::Assign,
                int_literal(1)
            )
        )
    ]);
}

#[test]
fn test_assign_to_index_expression() {
    parse_test("
obj['prop'] = 1
", vec![
        expression_statement(
            assign(
                lhs_index(identifier("obj".into()), string_literal("prop".into())),
                AssignmentOp::Assign,
                int_literal(1)
            )
        )
    ]);
}

#[test]
fn test_assign_to_chained_member_expression() {
    parse_test("
obj.a.b['prop'][0] = 1.0
", vec![
        expression_statement(
            assign(
                lhs_index(
                    index(
                        member(
                            member(
                                identifier("obj".into()),
                                identifier("a".into())
                            ),
                            identifier("b".into())
                        ),
                        string_literal("prop".into())
                    ),
                    int_literal(0)
                ),
                AssignmentOp::Assign,
                float32_literal(1.0)
            )
        )
    ]);
}

#[test]
fn test_function_call() {
    parse_test("
print(\"Hello\")
", vec![
        expression_statement(
            call(
                identifier("print".into()),
                vec![string_literal("Hello".into())]
            )
        )
    ]);
}

#[test]
fn test_chained_function_call() {
    parse_test("
func(0)()
", vec![
        expression_statement(
            call(
                call(
                    identifier("func".into()),
                    vec![int_literal(0)]
                ),
                vec![]
            )
        )
    ]);
}

#[test]
fn test_member_function_call() {
    parse_test("
coordinates.compute(x, y, z)
", vec![
        expression_statement(
            call(
                member(
                    identifier("coordinates".into()), 
                    identifier("compute".into())
                ),
                vec![identifier("x".into()), identifier("y".into()), identifier("z".into())]
            )
        )
    ]);
}

#[test]
fn test_use_statement_local_module() {
    parse_test("
// Local module 
use Calc
", vec![
        use_statement(
            import_path("Calc".into()),
            None
        )
    ]);
}

#[test]
fn test_use_statement_multiple_segments() {
    parse_test("
// Local module with path
use MyProject.Path.SomeModule
", vec![
        use_statement(
            import_path("MyProject.Path.SomeModule".into()),
            None
        )
    ]);
}

#[test]
fn test_use_statement_alias() {
    parse_test("
// Module with path
use System.Math as M
", vec![
        use_statement(
            import_path("System.Math".into()),
            opt_expr(identifier("M".into()))
        )
    ]);
}

#[test]
fn test_parse_list_type_in_variable() {
    parse_type_test(
        "[int]",
        typ(Type::List(Box::new(typ(Type::Int))))
    );
}

#[test]
fn test_parse_nullable_map_type_in_parameter() {
    parse_test("
def process_data(data {string: bool}?)
    // body
", vec![
        def(
            "process_data".into(),
            None,
            vec![
                parameter(
                    "data".into(),
                    opt_expr(
                        null_typ(
                            Type::Map(
                                Box::new(typ(Type::String)),
                                Box::new(typ(Type::Boolean))
                            ),
                        )
                    ),
                    None
                )
            ],
            None,
            empty_statement()
        )
    ]);
}

#[test]
fn test_parse_tuple_type_as_return_type() {
    parse_test("
def get_coordinates() (float, float?, float)?
    // body
", vec![
        def(
            "get_coordinates".into(),
            None,
            vec![],
            opt_expr(
                null_typ(
                    Type::Tuple(vec![
                        typ(Type::Float),
                        null_typ(Type::Float),
                        typ(Type::Float),
                    ]),
                )
            ),
            empty_statement()
        )
    ]);
}

#[test]
fn test_parse_generic_result_type() {
    parse_type_test(
        "result<int, string>", 
        typ(
            Type::Result(
                Box::new(typ(Type::Int)), 
                Box::new(typ(Type::String))
            )
        )
    );
}

#[test]
fn test_parse_generic_custom_type_with_nesting() {
    parse_test("
def get_data() MyContainer<[int]?, future<string>>
    // body
", vec![
        def(
            "get_data".into(),
            None,
            vec![],
            opt_expr(
                typ(
                    Type::Custom(
                        "MyContainer".to_string(),
                        Some(vec![
                            null_typ(Type::List(Box::new(typ(Type::Int)))), // [int]?
                            typ(Type::Future(Box::new(typ(Type::String)))) // future<string>
                        ])
                    )
                )
            ),
            empty_statement()
        )
    ]);
}

#[test]
fn test_parse_set_type() {
    parse_type_test(
        "{i64}", 
        typ(Type::Set(Box::new(typ(Type::I64))))
    );
}

#[test]
fn test_error_unclosed_list_type() {
    parse_error_test(
        "let my_list [int",
        SyntaxErrorKind::UnexpectedEOF
    );
}

#[test]
fn test_error_malformed_map_type() {
    parse_error_test(
        "let my_map {string, int}",
        SyntaxErrorKind::UnexpectedToken {
            expected: "}".to_string(),
            found: ",".to_string(),
        }
    );
}

#[test]
fn test_error_incomplete_generic_type() {
    parse_error_test(
        "let my_generic MyType<int,",
        SyntaxErrorKind::InvalidTypeDeclaration {
            expected: "Generic type".to_string(),
        }
    );
}

#[test]
fn test_error_empty_generic_parameters() {
    parse_error_test(
        "let my_generic MyType<>",
        SyntaxErrorKind::InvalidTypeDeclaration {
            expected: "Generic type".to_string()
        }
    );
}

#[test]
fn test_deeply_nested_collection_type() {
    parse_type_test(
        "[[{string: (int?, bool)}]?]?",
        null_typ( // The outer list is nullable: `[...]`?
            Type::List(Box::new(
                null_typ( // The inner list is nullable: `[{...}]?`
                    Type::List(Box::new(
                        typ(Type::Map( // The map itself is not nullable
                            Box::new(typ(Type::String)),
                            Box::new(typ(Type::Tuple(vec![
                                null_typ(Type::Int), // int?
                                typ(Type::Boolean)
                            ])))
                        ))
                    ))
                )
            ))
        )
    );
}

#[test]
fn test_unit_type_tuple() {
    parse_error_test(
        "let u ()",
        SyntaxErrorKind::InvalidTypeDeclaration {
            expected: "Tuple element type".to_string(),
        }
    );
}

#[test]
fn test_single_element_tuple_type() {
    parse_type_test(
        "(string)",
        typ(Type::Tuple(vec![typ(Type::String)]))
    );
}

#[test]
fn test_simple_nullable_built_in_type() {
    parse_type_test(
        "int?",
        null_typ(Type::Int)
    );
}

#[test]
fn test_error_trailing_comma_in_tuple_type() {
    parse_error_test(
        "let x (int, )",
        SyntaxErrorKind::InvalidTypeDeclaration {
            expected: "Tuple element type".to_string()
        }
    );
}

#[test]
fn test_error_map_missing_value_type() {
    parse_error_test(
        "let x {string:}",
        SyntaxErrorKind::InvalidTypeDeclaration {
            expected: "Map value type".to_string(),
        }
    );
}

#[test]
fn test_error_result_type_missing_parameter() {
    parse_error_test(
        "let x result<int>",
        SyntaxErrorKind::UnexpectedToken {
            expected: ",".to_string(),
            found: ">".to_string(),
        }
    );
}

#[test]
fn test_error_double_nullable() {
    parse_error_test(
        "let x int??",
        SyntaxErrorKind::UnexpectedToken {
            expected: "end of expression".to_string(),
            found: "?".to_string(),
        }
    );
}

#[test]
fn test_primitive_types() {
    let type_map = vec![
        ("int", Type::Int),
        ("i8", Type::I8),
        ("i16", Type::I16),
        ("i32", Type::I32),
        ("i64", Type::I64),
        ("i128", Type::I128),
        ("u8", Type::U8),
        ("u16", Type::U16),
        ("u32", Type::U32),
        ("u64", Type::U64),
        ("u128", Type::U128),
        ("float", Type::Float),
        ("f32", Type::F32),
        ("f64", Type::F64),
        ("string", Type::String),
        ("bool", Type::Boolean),
        ("symbol", Type::Symbol),
        ("result<int, string>", Type::Result(Box::new(typ(Type::Int)), Box::new(typ(Type::String)))),
        ("list<float>", Type::List(Box::new(typ(Type::Float)))),
        ("map<string, int>", Type::Map(Box::new(typ(Type::String)), Box::new(typ(Type::Int)))),
        ("set<string>", Type::Set(Box::new(typ(Type::String)))),
        ("future<string>", Type::Future(Box::new(typ(Type::String)))),
        ("tuple<string, int, float>", Type::Tuple(vec![typ(Type::String), typ(Type::Int), typ(Type::Float)]))
    ];
    for (name, mapped_type) in type_map {
        parse_type_test(name, typ(mapped_type.clone()));
        parse_type_test(format!("{}?", name).as_str(), null_typ(mapped_type.clone()));
    }
}

#[test]
fn test_type_alias_statement() {
    parse_test("
type MyInt is int
", vec![
        type_statement(vec![
            type_declaration(
                "MyInt",
                TypeDeclarationKind::Is,
                opt_expr(typ(Type::Int))
            )
        ])
    ]);
}

#[test]
fn test_type_alias_complex() {
    parse_test("
type UserMap is {string: User?}
", vec![
        type_statement(vec![
            type_declaration(
                "UserMap",
                TypeDeclarationKind::Is,
                opt_expr(typ(Type::Map(
                    Box::new(typ(Type::String)),
                    Box::new(null_typ(Type::Custom("User".into(), None)))
                )))
            )
        ])
    ]);
}

#[test]
fn test_type_parameter_unconstrained() {
    parse_test("
type T, U
", vec![
        type_statement(vec![
            type_declaration("T", TypeDeclarationKind::None, None),
            type_declaration("U", TypeDeclarationKind::None, None)
        ])
    ]);
}

#[test]
fn test_type_parameter_constrained() {
    parse_test("
type T extends SomeClass
", vec![
        type_statement(vec![
            type_declaration(
                "T",
                TypeDeclarationKind::Extends,
                opt_expr(typ(Type::Custom("SomeClass".into(), None)))
            )
        ])
    ]);
}

#[test]
fn test_type_parameter_list_mixed() {
    parse_test("
type T, U extends Serializable, X implements IGraph
", vec![
        type_statement(vec![
            type_declaration("T", TypeDeclarationKind::None, None),
            type_declaration(
                "U",
                TypeDeclarationKind::Extends,
                opt_expr(typ(Type::Custom("Serializable".into(), None)))
            ),
            type_declaration(
                "X",
                TypeDeclarationKind::Implements,
                opt_expr(typ(Type::Custom("IGraph".into(), None)))
            )
        ])
    ]);
}

#[test]
fn test_error_type_statement_missing_keyword() {
    parse_error_test(
        "type T SomeClass",
        SyntaxErrorKind::UnexpectedToken {
            expected: "is, implements, includes or extends".to_string(),
            found: "identifier".to_string(),
        }
    );
}

#[test]
fn test_error_type_statement_trailing_comma() {
    parse_error_test(
        "type T,",
        SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "end of file".to_string(),
        }
    );
}

#[test]
fn test_error_type_statement_missing_identifier() {
    parse_error_test(
        "type is int",
        SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "is".to_string(),
        }
    );
}

#[test]
fn test_break_in_for_loop() {
    parse_test("
for i in 1..10
    if i == 5
        break
", vec![
        for_statement(
            vec![let_variable("i", None, None)],
            range(int_literal(1), opt_expr(int_literal(10)), RangeExpressionType::Exclusive),
            block(vec![
                if_statement(
                    binary(identifier("i"), BinaryOp::Equal, int_literal(5)),
                    block(vec![break_statement()]),
                    None
                )
            ])
        )
    ]);
}

#[test]
fn test_continue_in_while_loop() {
    parse_test("
while x > 0
    if x == 1
        continue
    x -= 1
", vec![
        while_statement(
            binary(identifier("x"), BinaryOp::GreaterThan, int_literal(0)),
            block(vec![
                if_statement(
                    binary(identifier("x"), BinaryOp::Equal, int_literal(1)),
                    block(vec![continue_statement()]),
                    None
                ),
                expression_statement(
                    assign(
                        lhs_identifier("x"),
                        AssignmentOp::AssignSub,
                        int_literal(1)
                    )
                )
            ])
        )
    ]);
}

#[test]
fn test_break_in_forever_loop() {
    parse_test("
forever
    print('running')
    break
", vec![
        forever_statement(
            block(vec![
                expression_statement(
                    call(identifier("print"), vec![string_literal("running")])
                ),
                break_statement()
            ])
        )
    ]);
}

#[test]
fn test_break_in_nested_loop() {
    parse_test("
for i in 1..3
    for j in 1..3
        if j == 2
            break // breaks inner loop only
", vec![
        for_statement(
            vec![let_variable("i", None, None)],
            range(int_literal(1), opt_expr(int_literal(3)), RangeExpressionType::Exclusive),
            block(vec![
                for_statement(
                    vec![let_variable("j", None, None)],
                    range(int_literal(1), opt_expr(int_literal(3)), RangeExpressionType::Exclusive),
                    block(vec![
                        if_statement(
                            binary(identifier("j"), BinaryOp::Equal, int_literal(2)),
                            block(vec![break_statement()]),
                            None
                        )
                    ])
                )
            ])
        )
    ]);
}

#[test]
fn test_continue_in_nested_loop() {
    parse_test("
while a
    while b
        continue // continues inner loop only
", vec![
        while_statement(
            identifier("a"),
            block(vec![
                while_statement(
                    identifier("b"),
                    block(vec![continue_statement()])
                )
            ])
        )
    ]);
}

#[test]
fn test_error_break_with_value() {
    parse_error_test(
        "for x in y: break 1",
        SyntaxErrorKind::UnexpectedToken {
            expected: "end of expression".to_string(),
            found: "int".to_string(),
        }
    );
}

#[test]
fn test_error_continue_with_value() {
    parse_error_test(
        "while true: continue false",
        SyntaxErrorKind::UnexpectedToken {
            expected: "end of expression".to_string(),
            found: "false".to_string(),
        }
    );
}

// Note: `break` or `continue` outside a loop is a *semantic* error, not a *syntactic* one.
// The parser should successfully parse it, and a later analysis pass would reject it.
#[test]
fn test_parse_break_outside_loop() {
    parse_test("break", vec![break_statement()]);
}

#[test]
fn test_parse_continue_outside_loop() {
    parse_test("continue", vec![continue_statement()]);
}

#[test]
fn test_inline_enum_simple_values() {
    parse_test("
enum Colors: Red, Green, Blue
", vec![
        enum_statement(
            identifier("Colors"),
            vec![
                enum_value("Red", vec![]),
                enum_value("Green", vec![]),
                enum_value("Blue", vec![])
            ]
        )
    ]);
}

#[test]
fn test_block_enum_simple_values() {
    parse_test("
enum Colors
    Red
    Green
    Blue
", vec![
        enum_statement(
            identifier("Colors"),
            vec![
                enum_value("Red", vec![]),
                enum_value("Green", vec![]),
                enum_value("Blue", vec![])
            ]
        )
    ]);
}

#[test]
fn test_inline_enum_with_typed_values() {
    parse_test("
enum Message: Write(string), Move(int, int)
", vec![
        enum_statement(
            identifier("Message"),
            vec![
                enum_value("Write", vec![typ(Type::String)]),
                enum_value("Move", vec![typ(Type::Int), typ(Type::Int)])
            ]
        )
    ]);
}

#[test]
fn test_block_enum_with_mixed_values() {
    parse_test("
enum Event
    Quit
    KeyPress(int)
    Click(int, int)
", vec![
        enum_statement(
            identifier("Event"),
            vec![
                enum_value("Quit", vec![]),
                enum_value("KeyPress", vec![typ(Type::Int)]),
                enum_value("Click", vec![typ(Type::Int), typ(Type::Int)])
            ]
        )
    ]);
}

#[test]
fn test_enum_with_single_value() {
    parse_test("enum Status: Ok", vec![
        enum_statement(identifier("Status"), vec![enum_value("Ok", vec![])])
    ]);
}

#[test]
fn test_enum_with_complex_value_types() {
    parse_test("
enum Data: Point([int]?), Config({string: bool})
", vec![
        enum_statement(
            identifier("Data"),
            vec![
                enum_value("Point", vec![null_typ(Type::List(Box::new(typ(Type::Int))))]),
                enum_value("Config", vec![typ(Type::Map(Box::new(typ(Type::String)), Box::new(typ(Type::Boolean))))])
            ]
        )
    ]);
}

#[test]
fn test_empty_block_enum() {
    parse_error_test("
enum EmptyEnum
    // No values

let x = 0
", SyntaxErrorKind::UnexpectedToken {
        expected: "an indentation for block enums".to_string(),
        found: "let".to_string()
    });
}

#[test]
fn test_error_enum_missing_name() {
    parse_error_test(
        "enum: Red, Blue",
        SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: ":".to_string(),
        }
    );
}

#[test]
fn test_error_enum_missing_colon_or_indent() {
    parse_error_test(
        "enum Colors Red",
        SyntaxErrorKind::UnexpectedToken {
            expected: "either a colon for inline enums or an indentation for block enums".to_string(),
            found: "identifier".to_string(),
        }
    );
}

#[test]
fn test_error_enum_empty_inline() {
    parse_error_test(
        "enum Colors:",
        SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "end of file".to_string(),
        }
    );
}

#[test]
fn test_error_enum_malformed_value_type() {
    parse_error_test(
        "enum E: V(int,)",
        SyntaxErrorKind::InvalidTypeDeclaration {
            expected: "Enum value type".to_string(),
        }
    );
}

#[test]
fn test_inline_struct_simple_members() {
    parse_test("
struct Point: x int, y int
", vec![
        struct_statement(
            identifier("Point"),
            vec![
                struct_member("x", typ(Type::Int)),
                struct_member("y", typ(Type::Int))
            ]
        )
    ]);
}

#[test]
fn test_block_struct_simple_members() {
    parse_test("
struct Point
    x int
    y int
", vec![
        struct_statement(
            identifier("Point"),
            vec![
                struct_member("x", typ(Type::Int)),
                struct_member("y", typ(Type::Int))
            ]
        )
    ]);
}

#[test]
fn test_struct_with_complex_member_types() {
    parse_test("
struct UserProfile
    id string
    aliases [string]?
    preferences {string: bool}
", vec![
        struct_statement(
            identifier("UserProfile"),
            vec![
                struct_member("id", typ(Type::String)),
                struct_member("aliases", null_typ(Type::List(Box::new(typ(Type::String))))),
                struct_member("preferences", typ(Type::Map(Box::new(typ(Type::String)), Box::new(typ(Type::Boolean)))))
            ]
        )
    ]);
}

#[test]
fn test_struct_with_single_member() {
    parse_test("struct Wrapper: value float", vec![
        struct_statement(
            identifier("Wrapper"),
            vec![struct_member("value", typ(Type::Float))]
        )
    ]);
}

#[test]
fn test_empty_block_struct() {
    parse_error_test("
struct Empty
    // This struct has no members
", SyntaxErrorKind::UnexpectedToken {
        expected: "an indentation for block structs".to_string(),
        found: "end of file".to_string(),
    });
}

#[test]
fn test_error_struct_missing_name() {
    parse_error_test(
        "struct: x int",
        SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: ":".to_string(),
        }
    );
}

#[test]
fn test_error_struct_missing_colon_or_indent() {
    parse_error_test(
        "struct Point x int",
        SyntaxErrorKind::UnexpectedToken {
            expected: "either a colon for inline structs or an indentation for block structs".to_string(),
            found: "identifier".to_string(),
        }
    );
}

#[test]
fn test_error_struct_member_missing_type() {
    parse_error_test(
        "struct Point: x, y int",
        SyntaxErrorKind::MissingStructMemberType
    );
}

#[test]
fn test_error_struct_trailing_comma_inline() {
    parse_error_test(
        "struct Point: x int,",
        SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "end of file".to_string(),
        }
    );
}

#[test]
fn test_precedence_of_unary_not_and_logical_and() {
    // `not` should have higher precedence than `and`.
    // This should parse as `(not a) and b`.
    parse_test("not a and b", vec![
        expression_statement(
            logical(
                unary(UnaryOp::Not, identifier("a")),
                BinaryOp::And,
                identifier("b")
            )
        )
    ]);
}

#[test]
fn test_precedence_of_member_access_and_unary_negation() {
    // Member access `.` has higher precedence than unary `-`.
    // This should parse as `-(a.b)`.
    parse_test("-a.b", vec![
        expression_statement(
            unary(
                UnaryOp::Negate,
                member(identifier("a"), identifier("b"))
            )
        )
    ]);
}

#[test]
fn test_precedence_of_assignment_and_conditional_expression() {
    // The conditional expression has higher precedence than assignment.
    // This should parse as `x = (1 if y else 2)`.
    parse_test("x = 1 if y else 2", vec![
        expression_statement(
            assign(
                lhs_identifier("x"),
                AssignmentOp::Assign,
                if_conditional(
                    int_literal(1),
                    identifier("y"),
                    Some(int_literal(2))
                )
            )
        )
    ]);
}

#[test]
fn test_chained_calls_and_member_access() {
    // `a.b(c).d` should parse as `((a.b)(c)).d`
    parse_test("a.b(c).d", vec![
        expression_statement(
            member(
                call(
                    member(identifier("a"), identifier("b")),
                    vec![identifier("c")]
                ),
                identifier("d")
            )
        )
    ]);
}

#[test]
fn test_comment_between_function_name_and_params() {
    // This is unusual but should be syntactically valid.
    parse_test("
def my_func /* comment */ (a int)
    // body
", vec![
        def(
            "my_func".into(),
            None,
            vec![parameter("a".into(), opt_expr(typ(Type::Int)), None)],
            None,
            empty_statement()
        )
    ]);
}

#[test]
fn test_very_long_chain_of_binary_operators() {
    // Stress test the loop-based expression parsing to ensure it doesn't have performance issues
    // or stack overflows (which it shouldn't, but this is a good sanity check).

    // We don't need to build the full AST here, just confirm it parses without crashing.
    // A more dedicated test could build the deeply nested tree if desired.
    // For now, we just check that `parser.parse()` returns Ok.
    let long_expr = "1 + ".repeat(500) + "1";
    parse_program(&long_expr);
}

#[test]
fn test_namespaced_function_call() {
    parse_test("Http::new(url)", vec![
        expression_statement(
            call(
                class_identifier("Http::new"),
                vec![identifier("url")]
            )
        )
    ]);
}

#[test]
fn test_namespaced_enum_access() {
    parse_test("let status = Http::Status.Ok", vec![
        variable_statement(vec![
            let_variable(
                "status",
                None,
                opt_expr(member(
                    class_identifier("Http::Status"),
                    identifier("Ok")
                ))
            )
        ])
    ]);
}

#[test]
fn test_namespaced_type_in_variable_declaration() {
    parse_variable_declaration_test(
        "let client Http::Client",
        vec![
            let_variable(
                "client",
                opt_expr(typ(Type::Custom("Http::Client".into(), None))),
                None
            )
        ]
    );
}

#[test]
fn test_namespaced_type_in_function_return() {
    parse_test("def get_status() Http::Status: Http::Status.Ok", vec![
        def(
            "get_status".into(),
            None,
            vec![],
            opt_expr(typ(Type::Custom("Http::Status".into(), None))),
            expression_statement(
                member(
                    class_identifier("Http::Status"),
                    identifier("Ok")
                )
            )
        )
    ]);
}

#[test]
fn test_namespaced_type_in_function_parameter() {
    parse_test("def set_status(s Http::Status): _status = s", vec![
        def(
            "set_status".into(),
            None,
            vec![
                parameter(
                    "s".into(),
                    opt_expr(typ(Type::Custom("Http::Status".into(), None))),
                    None
                )
            ],
            None,
            expression_statement(
                assign(
                    lhs_identifier("_status"),
                    AssignmentOp::Assign,
                    identifier("s")
                )
            )
        )
    ]);
}

#[test]
fn test_error_namespaced_variable_declaration() {
    // A variable name cannot be namespaced.
    parse_error_test(
        "let Http::x = 1",
        SyntaxErrorKind::UnexpectedToken {
            expected: "a simple identifier".to_string(),
            found: "Http::x".to_string(),
        }
    );
}

#[test]
fn test_error_namespaced_parameter_name() {
    // A function parameter name cannot be namespaced.
    parse_error_test(
        "def my_func(Http::p int)",
        SyntaxErrorKind::UnexpectedToken {
            expected: "a simple identifier".to_string(),
            found: "Http::p".to_string(),
        }
    );
}

#[test]
fn test_error_namespaced_assignment_target() {
    // A namespaced identifier like `Http::Status` is a value, not a variable,
    // so it cannot be the direct target of an assignment.
    parse_error_test(
        "Http::Status = 'new_status'",
        SyntaxErrorKind::InvalidLeftHandSideExpression
    );
}

fn parse(input: &str) -> Result<Program, SyntaxError> {
    let mut lexer = Lexer::new(input);
    let mut parser = Parser::new(&mut lexer, input, AstFactory::new());
    
    parser.parse()
}

fn parse_program<'src>(input: &'src str) -> Program {
    parse(input).unwrap()
}

fn parse_test<'src>(input: &'src str, _expected_body: Vec<Statement>) {
    let program = parse_program(input);
    assert_eq!(program, Program {
        body: _expected_body
    }, "Parsing failed for input: {}", input);
}

fn parse_error_test<'src>(input: &'src str, _expected_error: SyntaxErrorKind) {
    let parse_result = parse(input);
    assert!(parse_result.is_err());
    assert_eq!(parse_result.unwrap_err().kind, _expected_error);
}

fn parse_variable_declaration_test(input: &str, expected: Vec<VariableDeclaration>) {
    parse_test(input, vec![
        Statement::Variable(expected)
    ]);
}

fn parse_literal_test(input: &str, expected: Literal) {
    parse_test(input, vec![
        Statement::Expression(Expression::Literal(expected))
    ]);
}

fn parse_integer_test(input: &str, expected: IntegerLiteral) {
    parse_literal_test(input, Literal::Integer(expected));
}

fn parse_float_test(input: &str, expected: FloatLiteral) {
    parse_literal_test(input, Literal::Float(expected));
}

fn parse_binary_expression_test(input: &str, left: Expression, op: BinaryOp, right: Expression) {
    parse_test(input, vec![
        Statement::Expression(Expression::Binary(Box::new(left), op, Box::new(right)))
    ]);
}

fn parse_assignment_expression_test(input: &str, left: LeftHandSideExpression, op: AssignmentOp, right: Expression) {
    parse_test(input, vec![
        Statement::Expression(Expression::Assignment(Box::new(left), op, Box::new(right)))
    ]);
}

fn parse_if_expression_test(input: &str, condition: Expression, then_block: Statement, else_block: Option<Statement>, if_statement_type: IfStatementType) {
    parse_test(input, vec![
        Statement::If(Box::new(condition), Box::new(then_block), else_block.map(Box::new), if_statement_type)
    ]);
}

fn parse_if_test(input: &str, condition: Expression, then_block: Statement, else_block: Option<Statement>) {
    parse_if_expression_test(input, condition.clone(), then_block.clone(), else_block.clone(), IfStatementType::If);
    parse_if_expression_test(input.replace("if", "unless").as_str(), condition, then_block, else_block, IfStatementType::Unless);
}

fn parse_unary_expression_test(input: &str, op: UnaryOp, right: Expression) {
    parse_test(input, vec![
        Statement::Expression(Expression::Unary(op, Box::new(right)))
    ]);
}

fn parse_while_expression_test(input: &str, condition: Expression, then_block: Statement, while_statement_type: WhileStatementType) {
    parse_test(input, vec![
        Statement::While(Box::new(condition), Box::new(then_block), while_statement_type)
    ]);
}

fn parse_while_test(input: &str, condition: Expression, then_block: Statement) {
    parse_while_expression_test(input, condition.clone(), then_block.clone(), WhileStatementType::While);
    parse_while_expression_test(input.replace("while", "until").as_str(), condition, then_block, WhileStatementType::Until);
}

fn parse_for_test(input: &str, variable_declarations: Vec<VariableDeclaration>, iterable: Expression, body: Statement) {
    parse_test(input, vec![
        Statement::For(variable_declarations, Box::new(iterable), Box::new(body))
    ]);
}

fn parse_type_test(type_str: &str, expected: Expression) {
    let input = format!("let x {}", type_str);
    parse_variable_declaration_test(&input, vec![
        let_variable(
            "x",
            opt_expr(expected),
            None
        )
    ]);
}