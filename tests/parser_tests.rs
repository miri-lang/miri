// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::ast::*;
use miri::lexer::{Lexer};
use miri::parser::Parser;
use miri::syntax_error::SyntaxErrorKind;

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
            expected: "a different token".into(),
            found: "RBracket".into() 
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
        lhs_expression("x".into()), 
        AssignmentOp::Assign, 
        int_literal(123)
    );
}


#[test]
fn test_parse_chained_assignment_expression() {
    parse_assignment_expression_test(
        "x = y = 123", 
        lhs_expression("x".into()), 
        AssignmentOp::Assign, 
        assign(
            lhs_expression("y".into()),
            AssignmentOp::Assign,
            int_literal(123)
        )
    );
}

#[test]
fn test_parse_increment_assignment_expression() {
    parse_assignment_expression_test(
        "x += 100", 
        lhs_expression("x".into()), 
        AssignmentOp::AssignAdd,
        int_literal(100)
    );
}

#[test]
fn test_parse_decrement_assignment_expression() {
    parse_assignment_expression_test(
        "x -= 200", 
        lhs_expression("x".into()), 
        AssignmentOp::AssignSub,
        int_literal(200)
    );
}

#[test]
fn test_parse_multiplication_assignment_expression() {
    parse_assignment_expression_test(
        "x *= 10", 
        lhs_expression("x".into()), 
        AssignmentOp::AssignMul,
        int_literal(10)
    );
}

#[test]
fn test_parse_division_assignment_expression() {
    parse_assignment_expression_test(
        "x /= 10", 
        lhs_expression("x".into()), 
        AssignmentOp::AssignDiv,
        int_literal(10)
    );
}

#[test]
fn test_parse_modulo_assignment_expression() {
    parse_assignment_expression_test(
        "x %= 10", 
        lhs_expression("x".into()), 
        AssignmentOp::AssignMod,
        int_literal(10)
    );
}

#[test]
fn test_parse_increment_chained_assignment_expression() {
    parse_assignment_expression_test(
        "x = y = z += 100",
        lhs_expression("x".into()),
        AssignmentOp::Assign,
        assign(
            lhs_expression("y".into()),
            AssignmentOp::Assign,
            assign(
                lhs_expression("z".into()),
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
            let_variable("x", None, Some(int_literal(5)))
        ]
    );
}

#[test]
fn test_parse_typed_variable_declaration() {
    parse_variable_declaration_test(
        "let x int = 5",
        vec![
            let_variable("x", Some("int".into()), Some(int_literal(5)))
        ]
    );
}

#[test]
fn test_parse_typed_variable_declaration_no_initializer() {
    parse_variable_declaration_test(
        "let x float",
        vec![
            let_variable("x", Some("float".into()), None)
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
            var("text", None, Some(string_literal("Hello, World!")))
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
            let_variable("y", None, Some(int_literal(10))),
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
                vec![var("bar", None, Some(int_literal(100)))]
            ),
            variable_statement(
                vec![let_variable("foo", None, Some(assign(lhs_expression("bar"), AssignmentOp::Assign, int_literal(200))))]
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
                lhs_expression("x"),
                AssignmentOp::Assign,
                int_literal(10)
            )
        )
    ]),
    Some(
        block(vec![
            expression_statement(
                assign(
                    lhs_expression("x"),
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
        expression_statement(assign(lhs_expression("x"), AssignmentOp::Assign, int_literal(10)))
    ]),
        Some(
            block(vec![
                expression_statement(assign(lhs_expression("x"), AssignmentOp::Assign, int_literal(20)))
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
                lhs_expression("x"),
                AssignmentOp::Assign,
                int_literal(10)
            )
        )
    ]),
    Some(
        expression_statement(
            assign(
                lhs_expression("x"),
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
            lhs_expression("x"),
            AssignmentOp::Assign,
            int_literal(10)
        )
    ),
    Some(
        block(vec![
            expression_statement(
                assign(
                    lhs_expression("x".into()),
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
                lhs_expression("x".into()),
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
                        lhs_expression("x".into()),
                        AssignmentOp::Assign,
                        int_literal(10)
                    )
                )
            ]),
            Some(
                block(vec![
                    expression_statement(
                        assign(
                            lhs_expression("x".into()),
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
                                    lhs_expression("x".into()),
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
                                lhs_expression("x".into()),
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
            lhs_expression("x".into()),
            AssignmentOp::Assign,
            int_literal(10)
        )
    ),
    Some(
            expression_statement(
                assign(
                    lhs_expression("x".into()),
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
            lhs_expression("x".into()),
            AssignmentOp::Assign,
            int_literal(10) 
        )
    ),
    Some(
            expression_statement(
                assign(
                    lhs_expression("x".into()),
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
                lhs_expression("x".into()),
                AssignmentOp::Assign,
                int_literal(10)
            )
        ),
        Some(
            if_statement(
                identifier("z".into()),
                expression_statement(
                    assign(
                        lhs_expression("x".into()),
                        AssignmentOp::Assign,
                        int_literal(20)
                    )
                ),
                Some(
                    expression_statement(
                        assign(
                            lhs_expression("x".into()),
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
            lhs_expression("x".into()),
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
            lhs_expression("x".into()),
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
            lhs_expression("y".into()),
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
                        lhs_expression("y".into()),
                        AssignmentOp::Assign,
                        int_literal(2)
                    )
                )
            ]),
            Some(block(vec![
                expression_statement(
                    assign(
                        lhs_expression("y".into()),
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
            let_variable("y".into(), None, Some(int_literal(10)))
        ])
    ]),
    Some(block(vec![
        variable_statement(vec![
            var("z".into(), None, Some(int_literal(20))),
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
                lhs_expression("x".into()),
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
            lhs_expression("x".into()),
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
                lhs_expression("x".into()),
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
                            lhs_expression("x".into()),
                            AssignmentOp::Assign,
                            int_literal(1)
                        )
                    )
                ]),
                None,
            ),
            expression_statement(
                assign(
                    lhs_expression("x".into()),
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
                Expression::Identifier("x".into()),
                block(vec![
                    expression_statement(
                        assign(
                            lhs_expression("x".into()),
                            AssignmentOp::Assign,
                            int_literal(1)
                        )
                    )
                ]),
                None
            ),
            expression_statement(
                assign(
                    lhs_expression("x".into()),
                    AssignmentOp::Assign,
                    int_literal(2)
                )
            )
        ]
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
fn test_while_expression() {
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
                lhs_expression("x"),
                AssignmentOp::AssignSub,
                int_literal(1)
            )
        )
    ])
    );
}

#[test]
fn test_while_expression_inline() {
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
            lhs_expression("x"),
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
                        lhs_expression("x"),
                        AssignmentOp::AssignAdd,
                        int_literal(1)
                    )
                )
            ]),
            Some(
                block(vec![
                    expression_statement(
                        assign(
                            lhs_expression("x"),
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
                Some(
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
                Some(
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
                Some(
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
                    lhs_expression("x"),
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


fn parse_test<'src>(input: &'src str, _expected_body: Vec<Statement>) {
    let mut lexer = Lexer::new(input);
    let mut parser = Parser::new(&mut lexer, input, AstFactory::new());
    let parse_result = parser.parse();

    let program = parse_result.unwrap();
    assert_eq!(program, Program {
        body: _expected_body
    }, "Parsing failed for input: {}", input);
}

fn parse_error_test<'src>(input: &'src str, _expected_error: SyntaxErrorKind) {
    let mut lexer = Lexer::new(input);
    let mut parser = Parser::new(&mut lexer, input, AstFactory::new());
    let parse_result = parser.parse();

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