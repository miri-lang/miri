use miri::ast::{Program, Literal, IntegerLiteral, FloatLiteral};
use miri::lexer::{Lexer};
use miri::parser::Parser;


#[test]
fn test_parse_integer_literal() {
    parser_test("42", Literal::Integer(IntegerLiteral::I8(42)));
    parser_test("12345", Literal::Integer(IntegerLiteral::I16(12345)));
    parser_test("1_234_567_890", Literal::Integer(IntegerLiteral::I32(1234567890)));
    parser_test("9_223_372_036_854_775_807", Literal::Integer(IntegerLiteral::I64(9223372036854775807)));

    parser_test("0b1_01_010", Literal::Integer(IntegerLiteral::I8(42)));
    parser_test("0xFF", Literal::Integer(IntegerLiteral::I16(255)));
    parser_test("0o77", Literal::Integer(IntegerLiteral::I8(63)));
    parser_test("0o1234567", Literal::Integer(IntegerLiteral::I32(342391)));
}

#[test]
fn test_parse_float_literal() {
    parser_test("3.14", Literal::Float(FloatLiteral::F32(3.14)));
    parser_test("1.797693134862315", Literal::Float(FloatLiteral::F64(1.797693134862315)));

    parser_test("1_000.0", Literal::Float(FloatLiteral::F32(1_000.0)));
    parser_test("1_000_000.123456789", Literal::Float(FloatLiteral::F64(1_000_000.123456789)));
    
    parser_test("1.0e10", Literal::Float(FloatLiteral::F32(1.0e10)));
    parser_test("6.67430e-11", Literal::Float(FloatLiteral::F32(6.67430e-11)));
}

#[test]
fn test_parse_float_literal_edge_cases() {
    // Precision edge cases
    parser_test("3.141592", Literal::Float(FloatLiteral::F32(3.141592))); // fits f32
    parser_test("3.1415927", Literal::Float(FloatLiteral::F32(3.1415927))); // still fits
    parser_test("3.14159265", Literal::Float(FloatLiteral::F64(3.14159265))); // too long for f32

    // Largest and smallest values
    parser_test("3.4028235e38", Literal::Float(FloatLiteral::F32(3.4028235e38))); // max f32
    parser_test("1.17549435e-38", Literal::Float(FloatLiteral::F32(1.17549435e-38))); // min normal f32
    parser_test("1.7976931348623157e308", Literal::Float(FloatLiteral::F64(1.7976931348623157e308))); // max f64
    parser_test("2.2250738585072014e-308", Literal::Float(FloatLiteral::F64(2.2250738585072014e-308))); // min normal f64

    // Zeros
    parser_test("0.0", Literal::Float(FloatLiteral::F32(0.0)));
    parser_test("0.000000", Literal::Float(FloatLiteral::F32(0.0)));

    // Underscore formatting
    parser_test("123_456.789", Literal::Float(FloatLiteral::F32(123_456.789)));
    parser_test("1_000_000.1234567", Literal::Float(FloatLiteral::F64(1_000_000.1234567)));
    parser_test("1_000_000.12345678", Literal::Float(FloatLiteral::F64(1_000_000.12345678))); // too long

    // Scientific notation variants
    parser_test("1.0e+10", Literal::Float(FloatLiteral::F32(1.0e10)));
    parser_test("1.0E10", Literal::Float(FloatLiteral::F32(1.0e10)));
    parser_test("1.0000001e10", Literal::Float(FloatLiteral::F32(1.0000001e10_f32))); // precision edge
    parser_test("9.999999e+37", Literal::Float(FloatLiteral::F32(9.999999e37))); // edge of f32

    // Negative exponent
    parser_test("1.0e-10", Literal::Float(FloatLiteral::F32(1.0e-10)));
    parser_test("6.02214076e-23", Literal::Float(FloatLiteral::F64(6.02214076e-23))); // Planck constant

    // Extreme edge underflow
    parser_test("1e-46", Literal::Float(FloatLiteral::F64(1e-46))); // below f32 subnormal
    parser_test("1e-39", Literal::Float(FloatLiteral::F32(1e-39))); // subnormal but fits
}

#[test]
fn test_parse_string_literal() {
    parser_test("'hello single quote'", Literal::String("hello single quote".to_string()));
    parser_test("\"hello double quote\"", Literal::String("hello double quote".to_string()));
}

#[test]
fn test_parse_boolean_literal() {
    parser_test("true", Literal::Boolean(true));
    parser_test("false", Literal::Boolean(false));
}

#[test]
fn test_parse_symbol_literal() {
    parser_test(":my_fancy_symbol", Literal::Symbol("my_fancy_symbol".to_string()));
}

fn parser_test<'src>(input: &'src str, _expected_body: Literal) {
    let mut lexer = Lexer::new(input);
    let mut parser = Parser::new(&mut lexer, input);
    let parse_result = parser.parse();

    let program = parse_result.unwrap();
    assert_eq!(program, Program {
        body: _expected_body
    });
}
