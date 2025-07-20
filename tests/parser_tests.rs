use miri::ast::{Expression, FloatLiteral, IntegerLiteral, Literal, Program, Statement};
use miri::lexer::{Lexer};
use miri::parser::Parser;


#[test]
fn test_parse_integer_literal() {
    parse_literal_test("42", Literal::Integer(IntegerLiteral::I8(42)));
    parse_literal_test("12345", Literal::Integer(IntegerLiteral::I16(12345)));
    parse_literal_test("1_234_567_890", Literal::Integer(IntegerLiteral::I32(1234567890)));
    parse_literal_test("9_223_372_036_854_775_807", Literal::Integer(IntegerLiteral::I64(9223372036854775807)));

    parse_literal_test("0b1_01_010", Literal::Integer(IntegerLiteral::I8(42)));
    parse_literal_test("0xFF", Literal::Integer(IntegerLiteral::I16(255)));
    parse_literal_test("0o77", Literal::Integer(IntegerLiteral::I8(63)));
    parse_literal_test("0o1234567", Literal::Integer(IntegerLiteral::I32(342391)));
}

#[test]
fn test_parse_float_literal() {
    parse_literal_test("3.14", Literal::Float(FloatLiteral::F32(3.14)));
    parse_literal_test("1.797693134862315", Literal::Float(FloatLiteral::F64(1.797693134862315)));

    parse_literal_test("1_000.0", Literal::Float(FloatLiteral::F32(1_000.0)));
    parse_literal_test("1_000_000.123456789", Literal::Float(FloatLiteral::F64(1_000_000.123456789)));
    
    parse_literal_test("1.0e10", Literal::Float(FloatLiteral::F32(1.0e10)));
    parse_literal_test("6.67430e-11", Literal::Float(FloatLiteral::F32(6.67430e-11)));
}

#[test]
fn test_parse_float_literal_edge_cases() {
    // Precision edge cases
    parse_literal_test("3.141592", Literal::Float(FloatLiteral::F32(3.141592))); // fits f32
    parse_literal_test("3.1415927", Literal::Float(FloatLiteral::F32(3.1415927))); // still fits
    parse_literal_test("3.14159265", Literal::Float(FloatLiteral::F64(3.14159265))); // too long for f32

    // Largest and smallest values
    parse_literal_test("3.4028235e38", Literal::Float(FloatLiteral::F32(3.4028235e38))); // max f32
    parse_literal_test("1.17549435e-38", Literal::Float(FloatLiteral::F32(1.17549435e-38))); // min normal f32
    parse_literal_test("1.7976931348623157e308", Literal::Float(FloatLiteral::F64(1.7976931348623157e308))); // max f64
    parse_literal_test("2.2250738585072014e-308", Literal::Float(FloatLiteral::F64(2.2250738585072014e-308))); // min normal f64

    // Zeros
    parse_literal_test("0.0", Literal::Float(FloatLiteral::F32(0.0)));
    parse_literal_test("0.000000", Literal::Float(FloatLiteral::F32(0.0)));

    // Underscore formatting
    parse_literal_test("123_456.789", Literal::Float(FloatLiteral::F32(123_456.789)));
    parse_literal_test("1_000_000.1234567", Literal::Float(FloatLiteral::F64(1_000_000.1234567)));
    parse_literal_test("1_000_000.12345678", Literal::Float(FloatLiteral::F64(1_000_000.12345678))); // too long

    // Scientific notation variants
    parse_literal_test("1.0e+10", Literal::Float(FloatLiteral::F32(1.0e10)));
    parse_literal_test("1.0E10", Literal::Float(FloatLiteral::F32(1.0e10)));
    parse_literal_test("1.0000001e10", Literal::Float(FloatLiteral::F32(1.0000001e10_f32))); // precision edge
    parse_literal_test("9.999999e+37", Literal::Float(FloatLiteral::F32(9.999999e37))); // edge of f32

    // Negative exponent
    parse_literal_test("1.0e-10", Literal::Float(FloatLiteral::F32(1.0e-10)));
    parse_literal_test("6.02214076e-23", Literal::Float(FloatLiteral::F64(6.02214076e-23))); // Planck constant

    // Extreme edge underflow
    parse_literal_test("1e-46", Literal::Float(FloatLiteral::F64(1e-46))); // below f32 subnormal
    parse_literal_test("1e-39", Literal::Float(FloatLiteral::F32(1e-39))); // subnormal but fits
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
42
'Hello'
", vec![
        Statement::Expression(Expression::Literal(Literal::Integer(IntegerLiteral::I8(42)))),
        Statement::Expression(Expression::Literal(Literal::String("Hello".to_string())))
    ]);
}

fn parse_test<'src>(input: &'src str, _expected_body: Vec<Statement>) {
    let mut lexer = Lexer::new(input);
    let mut parser = Parser::new(&mut lexer, input);
    let parse_result = parser.parse();

    let program = parse_result.unwrap();
    assert_eq!(program, Program {
        body: _expected_body
    });
}

fn parse_literal_test(input: &str, expected: Literal) {
    parse_test(input, vec![
        Statement::Expression(Expression::Literal(expected))
    ]);
}
