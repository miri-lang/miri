use miri::ast::{AssignmentOp, AstFactory, BinaryOp, Expression, FloatLiteral, IntegerLiteral, LeftHandSideExpression, Literal, Program, Statement, VariableDeclaration, VariableDeclarationType};
use miri::lexer::{Lexer};
use miri::parser::Parser;


#[test]
fn test_parse_integer_literal() {
    parse_integer_test("42", IntegerLiteral::I8(42));
    parse_integer_test("12345", IntegerLiteral::I16(12345));
    parse_integer_test("1_234_567_890", IntegerLiteral::I32(1234567890));
    parse_integer_test("9_223_372_036_854_775_807", IntegerLiteral::I64(9223372036854775807));

    parse_integer_test("0b1_01_010", IntegerLiteral::I8(42));
    parse_integer_test("0xFF", IntegerLiteral::I16(255));
    parse_integer_test("0o77", IntegerLiteral::I8(63));
    parse_integer_test("0o1234567", IntegerLiteral::I32(342391));
}

#[test]
fn test_parse_float_literal() {
    parse_float_test("3.14", FloatLiteral::F32(3.14));
    parse_float_test("1.797693134862315", FloatLiteral::F64(1.797693134862315));

    parse_float_test("1_000.0", FloatLiteral::F32(1_000.0));
    parse_float_test("1_000_000.123456789", FloatLiteral::F64(1_000_000.123456789));

    parse_float_test("1.0e10", FloatLiteral::F32(1.0e10));
    parse_float_test("6.67430e-11", FloatLiteral::F32(6.67430e-11));
}

#[test]
fn test_parse_float_literal_edge_cases() {
    // Precision edge cases
    parse_float_test("3.141592", FloatLiteral::F32(3.141592)); // fits f32
    parse_float_test("3.1415927", FloatLiteral::F32(3.1415927)); // still fits
    parse_float_test("3.14159265", FloatLiteral::F64(3.14159265)); // too long for f32

    // Largest and smallest values
    parse_float_test("3.4028235e38", FloatLiteral::F32(3.4028235e38)); // max f32
    parse_float_test("1.17549435e-38", FloatLiteral::F32(1.17549435e-38)); // min normal f32
    parse_float_test("1.7976931348623157e308", FloatLiteral::F64(1.7976931348623157e308)); // max f64
    parse_float_test("2.2250738585072014e-308", FloatLiteral::F64(2.2250738585072014e-308)); // min normal f64

    // Zeros
    parse_float_test("0.0", FloatLiteral::F32(0.0));
    parse_float_test("0.000000", FloatLiteral::F32(0.0));

    // Underscore formatting
    parse_float_test("123_456.789", FloatLiteral::F32(123_456.789));
    parse_float_test("1_000_000.1234567", FloatLiteral::F64(1_000_000.1234567));
    parse_float_test("1_000_000.12345678", FloatLiteral::F64(1_000_000.12345678)); // too long

    // Scientific notation variants
    parse_float_test("1.0e+10", FloatLiteral::F32(1.0e+10));
    parse_float_test("1.0E10", FloatLiteral::F32(1.0E10));
    parse_float_test("1.0000001e10", FloatLiteral::F32(1.0000001e10_f32)); // precision edge
    parse_float_test("9.999999e+37", FloatLiteral::F32(9.999999e37)); // edge of f32

    // Negative exponent
    parse_float_test("1.0e-10", FloatLiteral::F32(1.0e-10));
    parse_float_test("6.02214076e-23", FloatLiteral::F64(6.02214076e-23)); // Planck constant

    // Extreme edge underflow
    parse_float_test("1e-46", FloatLiteral::F64(1e-46)); // below f32 subnormal
    parse_float_test("1e-39", FloatLiteral::F32(1e-39)); // subnormal but fits
}

#[test]
fn test_parse_string_literal() {
    parse_literal_test("'hello single quote'", Literal::String("hello single quote".to_string()));
    parse_literal_test("\"hello double quote\"", Literal::String("hello double quote".to_string()));
}

#[test]
fn test_parse_boolean_literal() {
    parse_literal_test("true", Literal::Boolean(true));
    parse_literal_test("false", Literal::Boolean(false));
}

#[test]
fn test_parse_symbol_literal() {
    parse_literal_test(":my_fancy_symbol", Literal::Symbol("my_fancy_symbol".to_string()));
}

#[test]
fn test_parse_expressions() {
    parse_test("
123
'Hello World'
", vec![
        Statement::Expression(
            Expression::Literal(Literal::Integer(IntegerLiteral::I8(123)))
        ),
        Statement::Expression(
            Expression::Literal(Literal::String("Hello World".to_string()))
        )
    ]);
}

// #[test]
// fn test_parse_block() {
//     parse_test("
// f:
//     123
//     'Hello World'
// ", vec![
//         Statement::Block(vec![
//             Statement::Expression(
//                 Expression::Literal(Literal::Integer(IntegerLiteral::I8(123)))
//             ),
//             Statement::Expression(
//                 Expression::Literal(Literal::String("Hello World".to_string()))
//             )
//         ])
//     ]);
// }

#[test]
fn test_parse_binary_expression() {
    parse_binary_expression_test(
        "123 + 456",
        Expression::Literal(Literal::Integer(IntegerLiteral::I8(123))),
        BinaryOp::Add,
        Expression::Literal(Literal::Integer(IntegerLiteral::I16(456)))
    );
}

#[test]
fn test_parse_chained_binary_expression() {
    parse_binary_expression_test(
        "123 + 456 - 789",
        Expression::Binary(
            Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I8(123)))),
            BinaryOp::Add,
            Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I16(456))))
        ),
        BinaryOp::Sub,
        Expression::Literal(Literal::Integer(IntegerLiteral::I16(789)))
    );
}

#[test]
fn test_parse_chained_multiply_expression() {
    parse_binary_expression_test(
        "2 + 2 * 2",
        Expression::Literal(Literal::Integer(IntegerLiteral::I8(2))),
        BinaryOp::Add,
        Expression::Binary(
            Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I8(2)))),
            BinaryOp::Mul,
            Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I8(2))))
        )
    );
}

#[test]
fn test_parse_bitwise_and_expression() {
    parse_binary_expression_test(
        "1 + 2 & 2",
        Expression::Binary(
            Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I8(1)))),
            BinaryOp::Add,
            Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I8(2))))
        ),
        BinaryOp::BitwiseAnd,
        Expression::Literal(Literal::Integer(IntegerLiteral::I8(2)))
    );
}

#[test]
fn test_parse_bitwise_or_expression() {
    parse_binary_expression_test(
        "1 + 2 | 2",
        Expression::Binary(
            Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I8(1)))),
            BinaryOp::Add,
            Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I8(2))))
        ),
        BinaryOp::BitwiseOr,
        Expression::Literal(Literal::Integer(IntegerLiteral::I8(2)))
    );
}

#[test]
fn test_parse_bitwise_xor_expression() {
    parse_binary_expression_test(
        "1 + 2 ^ 2",
        Expression::Binary(
            Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I8(1)))),
            BinaryOp::Add,
            Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I8(2))))
        ),
        BinaryOp::BitwiseXor,
        Expression::Literal(Literal::Integer(IntegerLiteral::I8(2)))
    );
}


#[test]
fn test_parse_multiply_with_parentheses_expression() {
    parse_binary_expression_test(
        "(2 + 2) * 2",
        Expression::Binary(
            Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I8(2)))),
            BinaryOp::Add,
            Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I8(2))))
        ),
        BinaryOp::Mul,
        Expression::Literal(Literal::Integer(IntegerLiteral::I8(2)))
    );
}

#[test]
fn test_parse_simple_parentheses_expression() {
    parse_test("
(123)
", vec![
        Statement::Expression(
            Expression::Literal(Literal::Integer(IntegerLiteral::I8(123)))
        )
    ]);
}

#[test]
fn test_parse_assignment_expression() {
    parse_assignment_expression_test(
        "x = 123", 
        LeftHandSideExpression::Identifier("x".into()), 
        AssignmentOp::Assign, 
        Expression::Literal(Literal::Integer(IntegerLiteral::I8(123)))
    );
}


#[test]
fn test_parse_chained_assignment_expression() {
    parse_assignment_expression_test(
        "x = y = 123", 
        LeftHandSideExpression::Identifier("x".into()), 
        AssignmentOp::Assign, 
        Expression::Assignment(
            Box::new(LeftHandSideExpression::Identifier("y".into())),
            AssignmentOp::Assign,
            Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I8(123))))
        )
    );
}

#[test]
fn test_parse_increment_assignment_expression() {
    parse_assignment_expression_test(
        "x += 100", 
        LeftHandSideExpression::Identifier("x".into()), 
        AssignmentOp::AssignAdd,
        Expression::Literal(Literal::Integer(IntegerLiteral::I8(100)))
    );
}

#[test]
fn test_parse_decrement_assignment_expression() {
    parse_assignment_expression_test(
        "x -= 200", 
        LeftHandSideExpression::Identifier("x".into()), 
        AssignmentOp::AssignSub,
        Expression::Literal(Literal::Integer(IntegerLiteral::I16(200)))
    );
}

#[test]
fn test_parse_multiplication_assignment_expression() {
    parse_assignment_expression_test(
        "x *= 10", 
        LeftHandSideExpression::Identifier("x".into()), 
        AssignmentOp::AssignMul,
        Expression::Literal(Literal::Integer(IntegerLiteral::I8(10)))
    );
}

#[test]
fn test_parse_division_assignment_expression() {
    parse_assignment_expression_test(
        "x /= 10", 
        LeftHandSideExpression::Identifier("x".into()), 
        AssignmentOp::AssignDiv,
        Expression::Literal(Literal::Integer(IntegerLiteral::I8(10)))
    );
}

#[test]
fn test_parse_modulo_assignment_expression() {
    parse_assignment_expression_test(
        "x %= 10", 
        LeftHandSideExpression::Identifier("x".into()), 
        AssignmentOp::AssignMod,
        Expression::Literal(Literal::Integer(IntegerLiteral::I8(10)))
    );
}

#[test]
fn test_parse_increment_chained_assignment_expression() {
    parse_assignment_expression_test(
        "x = y = z += 100",
        LeftHandSideExpression::Identifier("x".into()),
        AssignmentOp::Assign,
        Expression::Assignment(
            Box::new(LeftHandSideExpression::Identifier("y".into())),
            AssignmentOp::Assign,
            Box::new(Expression::Assignment(
                Box::new(LeftHandSideExpression::Identifier("z".into())),
                AssignmentOp::AssignAdd,
                Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I8(100))))
            ))
        )
    );
}

#[test]
fn test_parse_variable_declaration() {
    parse_variable_declaration_test(
        "let x = 5",
        vec![VariableDeclaration {
            name: "x".into(),
            typ: None,
            initializer: Some(Expression::Literal(Literal::Integer(IntegerLiteral::I8(5)))),
            declaration_type: VariableDeclarationType::Immutable,
        }]
    );
}

#[test]
fn test_parse_typed_variable_declaration() {
    parse_variable_declaration_test(
        "let x int = 5",
        vec![VariableDeclaration {
            name: "x".into(),
            typ: Some("int".into()),
            initializer: Some(Expression::Literal(Literal::Integer(IntegerLiteral::I8(5)))),
            declaration_type: VariableDeclarationType::Immutable,
        }]
    );
}

#[test]
fn test_parse_typed_variable_declaration_no_initializer() {
    parse_variable_declaration_test(
        "let x float",
        vec![VariableDeclaration {
            name: "x".into(),
            typ: Some("float".into()),
            initializer: None,
            declaration_type: VariableDeclarationType::Immutable,
        }]
    );
}

#[test]
fn test_parse_variable_declaration_no_initializer() {
    parse_variable_declaration_test(
        "let x",
        vec![VariableDeclaration {
            name: "x".into(),
            typ: None,
            initializer: None,
            declaration_type: VariableDeclarationType::Immutable,
        }]
    );
}

#[test]
fn test_parse_mutable_variable_declaration() {
    parse_variable_declaration_test(
        "var text = \"Hello, World!\"",
        vec![VariableDeclaration {
            name: "text".into(),
            typ: None,
            initializer: Some(Expression::Literal(Literal::String("Hello, World!".to_string()))),
            declaration_type: VariableDeclarationType::Mutable,
        }]
    );
}

#[test]
fn test_parse_multiple_variable_declaration_no_initializer() {
    parse_variable_declaration_test(
        "let x, y, z",
        vec![VariableDeclaration {
            name: "x".into(),
            typ: None,
            initializer: None,
            declaration_type: VariableDeclarationType::Immutable,
        },
        VariableDeclaration {
            name: "y".into(),
            typ: None,
            initializer: None,
            declaration_type: VariableDeclarationType::Immutable,
        },
        VariableDeclaration {
            name: "z".into(),
            typ: None,
            initializer: None,
            declaration_type: VariableDeclarationType::Immutable,
        }]
    );
}

#[test]
fn test_parse_multiple_variable_declaration_mixed_initializer() {
    parse_variable_declaration_test(
        "let x, y = 10, z",
        vec![VariableDeclaration {
            name: "x".into(),
            typ: None,
            initializer: None,
            declaration_type: VariableDeclarationType::Immutable,
        },
        VariableDeclaration {
            name: "y".into(),
            typ: None,
            initializer: Some(Expression::Literal(Literal::Integer(IntegerLiteral::I8(10)))),
            declaration_type: VariableDeclarationType::Immutable,
        },
        VariableDeclaration {
            name: "z".into(),
            typ: None,
            initializer: None,
            declaration_type: VariableDeclarationType::Immutable,
        }]
    );
}

#[test]
fn test_parse_variable_declaration_and_assignment() {
    parse_test("
var bar = 100
let foo = bar = 200
",
        vec![
            Statement::Variable(vec![VariableDeclaration {
                name: "bar".into(),
                typ: None,
                initializer: Some(Expression::Literal(Literal::Integer(IntegerLiteral::I8(100)))),
                declaration_type: VariableDeclarationType::Mutable,
            }]),
            Statement::Variable(vec![VariableDeclaration {
                name: "foo".into(),
                typ: None,
                initializer: Some(Expression::Assignment(
                    Box::new(LeftHandSideExpression::Identifier("bar".into())),
                    AssignmentOp::Assign,
                    Box::new(Expression::Literal(Literal::Integer(IntegerLiteral::I16(200))))
                )),
                declaration_type: VariableDeclarationType::Immutable,
            }])
        ]
    );
}

fn parse_test<'src>(input: &'src str, _expected_body: Vec<Statement>) {
    let mut lexer = Lexer::new(input);
    let mut parser = Parser::new(&mut lexer, input, AstFactory::new());
    let parse_result = parser.parse();

    let program = parse_result.unwrap();
    assert_eq!(program, Program {
        body: _expected_body
    });
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