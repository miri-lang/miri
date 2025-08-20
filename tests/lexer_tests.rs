// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::{lexer::{Lexer, Token}, syntax_error::SyntaxErrorKind};


#[test]
fn test_empty_input() {
    lexer_test("", vec![]);
}

#[test]
fn test_whitespace_only() {
    lexer_test("   \t  \n  \r\n  ", vec![]);
}

#[test]
fn test_symbols_and_operators() {
    lexer_test(": => -> <- || == != >= <= > < = + - * / % , . ( ) [ ] { } | & ^ .. ..= += -= *= /= %= ~ -- ++ ?", vec![
        Token::Colon,
        Token::FatArrow,
        Token::Arrow,
        Token::LeftArrow,
        Token::Parallel,
        Token::Equal,
        Token::NotEqual,
        Token::GreaterThanEqual,
        Token::LessThanEqual,
        Token::GreaterThan,
        Token::LessThan,
        Token::Assign,
        Token::Plus,
        Token::Minus,
        Token::Star,
        Token::Slash,
        Token::Percent,
        Token::Comma,
        Token::Dot,
        Token::LParen,
        Token::RParen,
        Token::LBracket,
        Token::RBracket,
        Token::LBrace,
        Token::RBrace,
        Token::Pipe,
        Token::Ampersand,
        Token::Caret,
        Token::Range,
        Token::RangeInclusive,
        Token::AssignAdd,
        Token::AssignSub,
        Token::AssignMul,
        Token::AssignDiv,
        Token::AssignMod,
        Token::Tilde,
        Token::Decrement,
        Token::Increment,
        Token::QuestionMark,
    ]);
}

#[test]
fn test_operators_without_spaces() {
    lexer_test("a+=b*=c/=d%=e==f!=g>=h<=i", vec![
        Token::Identifier, Token::AssignAdd,
        Token::Identifier, Token::AssignMul,
        Token::Identifier, Token::AssignDiv,
        Token::Identifier, Token::AssignMod,
        Token::Identifier, Token::Equal,
        Token::Identifier, Token::NotEqual,
        Token::Identifier, Token::GreaterThanEqual,
        Token::Identifier, Token::LessThanEqual,
        Token::Identifier,
    ]);
}

#[test]
fn test_complex_assignment_chains() {
    lexer_test("a = b = c += d *= e", vec![
        Token::Identifier, Token::Assign,
        Token::Identifier, Token::Assign,
        Token::Identifier, Token::AssignAdd,
        Token::Identifier, Token::AssignMul,
        Token::Identifier,
    ]);
}

#[test]
fn test_ambiguous_operator_sequences() {
    // Should be parsed as -- and -
    lexer_test("---", vec![Token::Decrement, Token::Minus]);
    // Should be parsed as ++ and +
    lexer_test("+++", vec![Token::Increment, Token::Plus]);
    // Should be parsed as .. and .
    lexer_test("...", vec![Token::Range, Token::Dot]);
    // Should be parsed as two separate Pipe tokens, not one Parallel
    lexer_test("a | | b", vec![Token::Identifier, Token::Pipe, Token::Pipe, Token::Identifier]);
}

#[test]
fn test_very_long_identifier() {
    let long_name = "a".repeat(1000);
    lexer_test(&long_name, vec![Token::Identifier]);
}

#[test]
fn test_number_identifier_boundaries() {
    lexer_test("123abc abc123 123.456fn", vec![
        Token::Int, Token::Identifier,
        Token::Identifier,
        Token::Float, Token::Fn,
    ]);
}

#[test]
fn test_unicode_identifiers() {
    lexer_error_test("café naïve résumé", SyntaxErrorKind::InvalidToken);
}

#[test]
fn test_keywords() {
    let keyword_map = vec![
        ("use", Token::Use),
        ("fn", Token::Fn),
        ("async", Token::Async),
        ("await", Token::Await),
        ("spawn", Token::Spawn),
        ("gpu", Token::Gpu),
        ("if", Token::If),
        ("unless", Token::Unless),
        ("match", Token::Match),
        ("default", Token::Default),
        ("return", Token::Return),
        ("while", Token::While),
        ("until", Token::Until),
        ("do", Token::Do),
        ("for", Token::For),
        ("in", Token::In),
        ("let", Token::Let),
        ("var", Token::Var),
        ("or", Token::Or),
        ("and", Token::And),
        ("not", Token::Not),
        ("true", Token::True),
        ("false", Token::False),
        ("from", Token::From),
        ("as", Token::As),
        ("break", Token::Break),
        ("continue", Token::Continue),
        ("is", Token::Is),
        ("extends", Token::Extends),
        ("includes", Token::Includes),
        ("implements", Token::Implements),
        ("type", Token::Type),
        ("enum", Token::Enum),
        ("struct", Token::Struct),
    ];

    for (keyword, token) in keyword_map {
        keyword_test(keyword, token);
    }
}

fn keyword_test(keyword: &str, expected: Token) {
    lexer_test(
        format!("{keyword} {keyword}() {keyword}.blah blah.{keyword} {keyword}blah blah{keyword} blah_{keyword} \"{keyword}\" /* {keyword} */").as_str(),
        vec![
        expected.clone(),
        expected.clone(), Token::LParen, Token::RParen,
        expected.clone(), Token::Dot, Token::Identifier,
        Token::Identifier, Token::Dot, expected.clone(),
        Token::Identifier,
        Token::Identifier,
        Token::Identifier,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_in_keyword() {
    lexer_test(
        "in not",
        vec![
        Token::In,
        Token::Not,
    ]);
}

#[test]
fn test_else_keyword() {
    lexer_test(
        "else else() else.blah blah.else blah blahelse blah_else \"else\" /* else */",
        vec![
        Token::Else,
        Token::ExpressionStatementEnd, Token::Else, Token::LParen, Token::RParen,
        Token::ExpressionStatementEnd, Token::Else, Token::Dot, Token::Identifier,
        Token::Identifier, Token::Dot, Token::ExpressionStatementEnd, Token::Else,
        Token::Identifier,
        Token::Identifier,
        Token::Identifier,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_literals_and_identifiers() {
    lexer_test("hello :name 'world' \"test\" 123 123_456 3.14 1_000.5_0", vec![
        Token::Identifier,
        Token::Symbol,
        Token::SingleQuotedString,
        Token::DoubleQuotedString,
        Token::Int,
        Token::Int,
        Token::Float,
        Token::Float,
    ]);
}

#[test]
fn test_strings_with_escapes() {
    lexer_test(r#"'string with \' quote' "string with \" quote""#, vec![
        Token::SingleQuotedString,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_empty_strings() {
    lexer_test("'' \"\"", vec![
        Token::SingleQuotedString,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_multiline_strings() {
    lexer_test("'line1\nline2' \"line1\nline2\"", vec![
        Token::SingleQuotedString,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_mixed_quotes_in_strings() {
    lexer_test(r#"'string with "double" quotes' "string with 'single' quotes""#, vec![
        Token::SingleQuotedString,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_unicode_strings() {
    lexer_test(r#""Hello 世界" "🚀 rocket""#, vec![
        Token::DoubleQuotedString,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_string_escape_sequences() {
    lexer_test(r#"'line1\nline2\ttab' "quote\"inside""#, vec![
        Token::SingleQuotedString,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_string_with_uncommon_escapes() {
    // Test escapes for backslash and different quote types
    lexer_test(r#""a \\ b" 'c \' d' "e \" f""#, vec![
        Token::DoubleQuotedString,
        Token::SingleQuotedString,
        Token::DoubleQuotedString,
    ]);
}

#[test]
fn test_unclosed_string_literal() {
    // An unclosed string should likely be tokenized up to the end of the line
    // and not consume the rest of the file.
    lexer_error_test("'unclosed string", SyntaxErrorKind::InvalidToken);
}

#[test]
fn test_number_edge_cases() {
    lexer_test("0 00 1_000_000 0.0 .5 5. -19 1.0e10 6.67430e-11 1E10 1e-5 1.5E+3 1.5e-10 1_000e10", vec![
        Token::Int,
        Token::Int,
        Token::Int,
        Token::Float,
        Token::Dot, Token::Int,  // .5 should be parsed as . and 5
        Token::Int, Token::Dot,  // 5. should be parsed as 5 and .,
        Token::Minus, Token::Int, // -19 should be parsed as Minus and 19
        // Scientific notation
        Token::Float,
        Token::Float,
        Token::Float,
        Token::Float,
        Token::Float,
        Token::Float,
        Token::Float,
    ]);
}

#[test]
fn test_float_precision_boundaries() {
    lexer_test("3.4028235e38 1.7976931348623157e308", vec![
        Token::Float, // f32 max
        Token::Float, // f64 max
    ]);
}

#[test]
fn test_integer_overflow_edge_cases() {
    lexer_test("9223372036854775807 9223372036854775808", vec![
        Token::Int, // i64::MAX
        Token::Int, // Should still tokenize, even if out of i64 range
    ]);
}

#[test]
fn test_very_large_numbers() {
    lexer_test("999999999999999999999999999999", vec![
        Token::Int, // Should tokenize even if unparseable
    ]);
}

#[test]
fn test_underscore_in_numbers() {
    lexer_test("1_2_3 4_5.6_7 1_234_567_890", vec![
        Token::Int,
        Token::Float,
        Token::Int,
    ]);
}

#[test]
fn test_binary_hex_octal_numbers() {
    lexer_test("0b1010 0x1A2B 0x1fff 0o755", vec![
        Token::BinaryNumber,
        Token::HexNumber,
        Token::HexNumber,
        Token::OctalNumber,
    ]);
}

#[test]
fn test_binary_hex_octal_numbers_with_underscores() {
    lexer_test("0b1010_1010 0b1_0_1_0_1_0_1_0 0b_1111 0x1_A2_B 0xFaFa_EeEe 0x_abcd 0o7_5_5 0o755_7777 0o_777", vec![
        Token::BinaryNumber,
        Token::BinaryNumber,
        Token::BinaryNumber,
        Token::HexNumber,
        Token::HexNumber,
        Token::HexNumber,
        Token::OctalNumber,
        Token::OctalNumber,
        Token::OctalNumber,
    ]);
}

#[test]
fn test_binary_hex_octal_numbers_incomplete() {
    lexer_test("0b 0x 0o", vec![
        Token::Int, // should not panic, just return other tokens
        Token::Identifier,
        Token::Int,
        Token::Identifier,
        Token::Int,
        Token::Identifier,
    ]);
}

// Note: this works, but maybe it shouldn't?
#[test]
fn test_binary_hex_octal_numbers_long_underscores() {
    lexer_test("0b___________ 0x___________ 0o___________", vec![
        Token::BinaryNumber, // should not panic, just return other tokens
        Token::HexNumber,
        Token::OctalNumber,
    ]);
}

#[test]
fn test_invalid_binary() {
    lexer_test("0b2 0bbb b111 0b1111_000F", vec![
        Token::Int,
        Token::Identifier,
        Token::Int,
        Token::Identifier,
        Token::Identifier,
        Token::BinaryNumber,
        Token::Identifier,
    ]);
}

#[test]
fn test_invalid_hex() {
    lexer_test("0xPPPPp 0xxxx x00 0x0123z", vec![
        Token::Int,
        Token::Identifier,
        Token::Int,
        Token::Identifier,
        Token::Identifier,
        Token::HexNumber,
        Token::Identifier,
    ]);
}

#[test]
fn test_invalid_octal() {
    lexer_test("0o8 0o9 0o7777z o7777", vec![
        Token::Int,
        Token::Identifier,
        Token::Int,
        Token::Identifier,
        Token::OctalNumber,
        Token::Identifier,
        Token::Identifier,
    ]);
}

#[test]
fn test_numbers_starting_with_dot() {
    // TODO: Should this be allowed? Works in Python, but not in Rust.
    lexer_test(".123", vec![Token::Dot, Token::Int]);
}

#[test]
fn test_numbers_ending_with_dot() {
    // TODO: Should this be allowed? Works in Python and Rust.
    lexer_test("123.", vec![Token::Int, Token::Dot]);
}

#[test]
fn test_hex_octal_binary_case_insensitivity() {
    lexer_test("0X1A 0B101 0O77", vec![
        Token::HexNumber,
        Token::BinaryNumber,
        Token::OctalNumber,
    ]);
}

#[test]
fn test_symbol_identifier_boundaries() {
    lexer_test(":symbol identifier: :another", vec![
        Token::Symbol,
        Token::Identifier,
        Token::Colon,
        Token::Symbol,
    ]);
}

#[test]
fn test_symbols_with_numbers() {
    lexer_test(":symbol123 :test_2 :_private", vec![
        Token::Symbol,
        Token::Symbol,
        Token::Symbol,
    ]);
}

#[test]
fn test_keywords_as_parts_of_identifiers() {
    lexer_test("if_condition use_case return_value", vec![
        Token::Identifier,  // should not be parsed as "if"
        Token::Identifier,  // should not be parsed as "use"
        Token::Identifier,  // should not be parsed as "return"
    ]);
}

#[test]
fn test_use() {
    lexer_test("
// Local module 
use Calc

// Global module
use System.Math

// Local module with path
use MyProject.Path.SomeModule

// Local module with path and alias
use Module2 as M2
", vec![
        Token::Use, Token::Identifier, Token::ExpressionStatementEnd,
        Token::Use, Token::Identifier, Token::Dot, Token::Identifier, Token::ExpressionStatementEnd,
        Token::Use, Token::Identifier, Token::Dot, Token::Identifier, Token::Dot, Token::Identifier, Token::ExpressionStatementEnd,
        Token::Use, Token::Identifier, Token::As, Token::Identifier, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_inline_comments() {
    lexer_test(r#"
var x = 10 // simple inline comment

print('Hello') // 👋 this is a friendly comment

use System.Math // use System.Math // with another comment inside

x = x + 1 // math: x becomes x + 1
"#, vec![
        Token::Var, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
        Token::Identifier, Token::LParen, Token::SingleQuotedString, Token::RParen, Token::ExpressionStatementEnd,
        Token::Use, Token::Identifier, Token::Dot, Token::Identifier, Token::ExpressionStatementEnd,
        Token::Identifier, Token::Assign, Token::Identifier, Token::Plus, Token::Int, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_multiline_comments() {
    lexer_test(r#"
/**/

/* This is a single-line comment */

/*****************************************/

/* This is a basic
multiline comment
spanning three lines */
let some = "code"

/* Multiline comment with code inside:
var a = 5
print('ignored!')
*/

fn func() int: 10 + 10

/***
/* 
  /* nested */ 
*/ 
***/

/*

  |\_/|
  ( o.o )   <- Cat!
  > ^ <

This is a comment with ASCII art.

Symbols: /* nested? */ < > & ^ ~
*/

print("Hello") /* inline comment */
"#, vec![
        Token::Let, Token::Identifier, Token::Assign, Token::DoubleQuotedString, Token::ExpressionStatementEnd,
        Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::Identifier, Token::Colon,
            Token::Int, Token::Plus, Token::Int, Token::ExpressionStatementEnd,
        Token::Identifier, Token::LParen, Token::DoubleQuotedString, Token::RParen, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_deeply_nested_comments() {
    lexer_test("before /* outer /* inner /* deepest */ inner */ outer */ after", vec![
        Token::Identifier, Token::Identifier
    ]);
}

#[test]
fn test_unclosed_nested_comment() {
    lexer_error_test("/* outer /* inner */ still open", SyntaxErrorKind::UnclosedMultilineComment);
}

#[test]
fn test_comment_with_code_like_content() {
    lexer_test("/* func(): if else */ real_code", vec![
        Token::Identifier
    ]);
}

#[test]
fn test_comment_at_eof() {
    lexer_test("code // comment with no newline", vec![
        Token::Identifier,
    ]);
}

#[test]
fn test_nested_comments_with_strings() {
    lexer_test(r#"/* outer /* "string inside comment" */ outer */ code"#, vec![
        Token::Identifier,
    ]);
}

#[test]
fn test_declaration() {
    lexer_test("
let x = 10                                   // inferred
var y = 20                                   // mutable
let z int = 30                               // explicitly typed
let num = 5.0                                // float
let str string = 'Hello'                     // string
let is_active = true                         // boolean
let even = 10 % 2 == 0                       // even number check
let m = Map<string, int>()                   // map declaration
let arr1 = [10, 20, 30]                      // array
let arr2 [float] = [1.0, 2.0, 3.0]           // array with type
let dict1 = {key1: 'A', key2: 'B'}           // dictionary
let dict2 {string: int} = {key1: 1, key2: 2} // dictionary with type
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
        Token::Var, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::Float, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Identifier, Token::Assign, Token::SingleQuotedString, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::True, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::Percent, Token::Int, Token::Equal, Token::Int, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::Identifier, Token::LessThan, Token::Identifier, Token::Comma, Token::Identifier, Token::GreaterThan, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::LBracket, Token::Int, Token::Comma, Token::Int, Token::Comma, Token::Int, Token::RBracket, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::LBracket, Token::Identifier, Token::RBracket, Token::Assign, Token::LBracket, Token::Float, Token::Comma, Token::Float, Token::Comma, Token::Float, Token::RBracket, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::LBrace, Token::Identifier, Token::Colon, Token::SingleQuotedString, Token::Comma, Token::Identifier, Token::Colon, Token::SingleQuotedString, Token::RBrace, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::LBrace, Token::Identifier, Token::Colon, Token::Identifier, Token::RBrace, Token::Assign, Token::LBrace, Token::Identifier, Token::Colon, Token::Int, Token::Comma, Token::Identifier, Token::Colon, Token::Int, Token::RBrace, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_function_with_no_params() {
    lexer_test("
// Function with no parameters
fn fancy_print
  print \"Hello, World!\"
", vec![
        Token::Fn, Token::Identifier, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::DoubleQuotedString, Token::ExpressionStatementEnd,
            Token::Dedent,
    ]);
}

#[test]
fn test_function_with_params() {
    lexer_test("
/* Function with parameters */
fn square(x int) int
  x * x

/* Another function example */
fn add(a int, b int) int
  a + b
", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::Star, Token::Identifier, Token::ExpressionStatementEnd,
            Token::Dedent,

        Token::Fn, Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::Comma, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::Plus, Token::Identifier, Token::ExpressionStatementEnd,
            Token::Dedent,
    ]);
}

#[test]
fn test_inline_function() {
    lexer_test("
// Inline function
fn multiply(a int, b int) int: a * b
", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::Comma, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::Colon, Token::Identifier, Token::Star, Token::Identifier, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_lambda_function() {
    lexer_test("
// Lambda function
let f = (x int) int: x * x
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::Colon, Token::Identifier, Token::Star, Token::Identifier, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_multiline_lambda_function() {
    lexer_test("
// Multiline lambda function
let f1 = (a float, b float)
  print(a + b)
  print(a - b)
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::LParen, Token::Identifier, Token::Identifier, Token::Comma, Token::Identifier, Token::Identifier, Token::RParen, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::LParen, Token::Identifier, Token::Plus, Token::Identifier, Token::RParen, Token::ExpressionStatementEnd,
            Token::Identifier, Token::LParen, Token::Identifier, Token::Minus, Token::Identifier, Token::RParen, Token::ExpressionStatementEnd,
            Token::Dedent,
    ]);
}

#[test]
fn test_function_calls_with_parentheses() {
    lexer_test("
// Call with parentheses
fancy_print()
f(10)
f1(5.0, 3.0)
", vec![
        Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
        Token::Identifier, Token::LParen, Token::Int, Token::RParen, Token::ExpressionStatementEnd,
        Token::Identifier, Token::LParen, Token::Float, Token::Comma, Token::Float, Token::RParen, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_function_calls_without_parentheses() {
    lexer_test("
// Call without parentheses
fancy_print
f 10
f1 5.0, 3.0
", vec![
        Token::Identifier, Token::ExpressionStatementEnd,
        Token::Identifier, Token::Int, Token::ExpressionStatementEnd,
        Token::Identifier, Token::Float, Token::Comma, Token::Float, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_function_call_with_codeblock() {
    lexer_test("
// Code block
let y = arr.map(
  fn (x int): x * 2
)
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::Identifier, Token::Dot, Token::Identifier, Token::LParen,
            Token::Fn, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Colon, Token::Identifier, Token::Star, Token::Int,
        Token::RParen, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_nested_function() {
    lexer_test("
// Nested function
fn nested_func(a int) int
  fn inner_func(x int) int
    print(x)
    let res = x + 1
    for i in 0..x
      print(i)
    print(res)
  inner_func(a)

nested_func(5)
", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Fn, Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::ExpressionStatementEnd,
                Token::Indent,
                Token::Identifier, Token::LParen, Token::Identifier, Token::RParen, Token::ExpressionStatementEnd,
                Token::Let, Token::Identifier, Token::Assign, Token::Identifier, Token::Plus, Token::Int, Token::ExpressionStatementEnd,
                Token::For, Token::Identifier, Token::In, Token::Int, Token::Range, Token::Identifier, Token::ExpressionStatementEnd,
                    Token::Indent,
                    Token::Identifier, Token::LParen, Token::Identifier, Token::RParen, Token::ExpressionStatementEnd,
                    Token::Dedent,
                Token::Identifier, Token::LParen, Token::Identifier, Token::RParen, Token::ExpressionStatementEnd,
                Token::Dedent,
            Token::Identifier, Token::LParen, Token::Identifier, Token::RParen, Token::ExpressionStatementEnd,
            Token::Dedent,

        Token::Identifier, Token::LParen, Token::Int, Token::RParen, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_windows_line_endings() {
    lexer_test("line1\r\nline2\r\n", vec![
        Token::Identifier, Token::ExpressionStatementEnd,
        Token::Identifier, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_mixed_whitespace_types() {
    lexer_test("fn func()\n\t  mixed_indent", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::Dedent,
    ]);
}

#[test]
fn test_uneven_indent_spaces() {
    lexer_error_test("
// Uneven spaces
fn func()
   fn three_spaces()
     fn two_spaces()
      one_space():
    fn four_spaces()
      print(\"Hello\")
  print(\"World\")
", SyntaxErrorKind::IndentationMismatch);
}

#[test]
fn test_uneven_indent_tabs() {
    lexer_error_test("
fn func()
\tfn tab()
\t\t\tfn tab()
\t\tfn tab()
print(\"Hello\")
", SyntaxErrorKind::IndentationMismatch);
}

#[test]
fn test_uneven_indent_spaces_tabs() {
    lexer_error_test("
// Mixed tabs and spaces
fn func()
\t\t\tfn tab()
\t\t\t print(\"Hello\")
  print(\"World\")
  \t\t\tfn tab()
    print(\"Indented with tabs\")
  print(\"Dedented with spaces\")
", SyntaxErrorKind::IndentationMismatch);
}

#[test]
fn test_indent_dedent_func() {
    lexer_test("
// Indented call
func(10,
     \"hello\",
     50)
", vec![
        Token::Identifier, Token::LParen, Token::Int, Token::Comma,            
            Token::DoubleQuotedString, Token::Comma,
            Token::Int, Token::RParen, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_indent_dedent_func_nested() {
    lexer_test("
// Indented call with nested indentation
func(10,
     50,
     fn nested_func(x int) int
       print(x)
       fn another_func(y int) int
         print(y)
         return y + 1
       return x + another_func(1))
", vec![
        Token::Identifier, Token::LParen, Token::Int, Token::Comma,
            Token::Int, Token::Comma,
            Token::Fn, Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::ExpressionStatementEnd,
                Token::Indent,
                Token::Identifier, Token::LParen, Token::Identifier, Token::RParen, Token::ExpressionStatementEnd,
                Token::Fn, Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::ExpressionStatementEnd,
                    Token::Indent,
                    Token::Identifier, Token::LParen, Token::Identifier, Token::RParen, Token::ExpressionStatementEnd,
                    Token::Return, Token::Identifier, Token::Plus, Token::Int, Token::ExpressionStatementEnd,
                    Token::Dedent,
                Token::Return, Token::Identifier, Token::Plus, Token::Identifier, Token::LParen, Token::Int, Token::RParen, Token::RParen, Token::ExpressionStatementEnd,
                Token::Dedent,
    ]);
}

#[test]
fn test_indent_dedent_func_arg_new_lines() {
    lexer_test("
// Indented call with all arguments on new lines
func(
  10,
  50
)    
", vec![
        Token::Identifier, Token::LParen,
            Token::Int, Token::Comma,
            Token::Int,
        Token::RParen, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_empty_lines_preserve_indentation_context() {
    lexer_test("
fn func()
    statement1

    statement2
        nested
    statement3
", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::ExpressionStatementEnd,
            Token::Identifier, Token::ExpressionStatementEnd,
                Token::Indent,
                Token::Identifier, Token::ExpressionStatementEnd,
                Token::Dedent,
            Token::Identifier, Token::ExpressionStatementEnd,
            Token::Dedent,
    ]);
}

#[test]
fn test_empty_lines_dont_prevent_dedent() {
    lexer_test("
statement1
  statement2
    statement3
  statement4

statement5

", vec![
        Token::Identifier, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::ExpressionStatementEnd,
                Token::Indent,
                Token::Identifier, Token::ExpressionStatementEnd,
                Token::Dedent,
            Token::Identifier, Token::ExpressionStatementEnd,
            Token::Dedent,

        Token::Identifier, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_multiple_dedent_levels() {
    lexer_test("
fn func()
    fn level1()
        fn level2()
            level3
back_to_root
", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
                Token::Indent,
                Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
                    Token::Indent,
                    Token::Identifier, Token::ExpressionStatementEnd,
                    Token::Dedent,
                Token::Dedent,
            Token::Dedent,
        Token::Identifier, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_indent_dedent_comments() {
    lexer_test("
     // this is just a comment

// still a comment

  /*
    /* and this is another comment 
      */
*/


  // Comment 1
    // Comment 2
        // Comment 3
      // Comment 4
        // Comment 5
// Comment 6
", vec![]);
}

#[test]
fn test_indentation_within_brackets_is_ignored() {
    lexer_test("
let x = [
    1,
    2,
]
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::LBracket,
        Token::Int, Token::Comma,
        Token::Int, Token::Comma,
        Token::RBracket, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_indentation_within_braces_is_ignored() {
    lexer_test("
let y = {
    'key': 'value',
    'another': 123
}
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::LBrace,
        Token::SingleQuotedString, Token::Colon, Token::SingleQuotedString, Token::Comma,
        Token::SingleQuotedString, Token::Colon, Token::Int,
        Token::RBrace, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_indented_line_with_only_a_comment() {
    lexer_test("
fn my_func()
    // This line is just a comment.
    let x = 1
", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
            Token::Dedent,
    ]);
}

#[test]
fn test_indented_line_with_multiple_inline_comments() {
    lexer_test("
fn my_func()
    // This line is just a comment.
    // Another comment
    let x = 1
", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
            Token::Dedent,
    ]);
}

#[test]
fn test_indented_line_with_multiline_comment() {
    lexer_test("
fn my_func()
    /*
        This line is just a comment.
        It spans multiple lines.
        // and it has inline comments too.
        /* even nested comments */
    */
    let x = 1
", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
            Token::Dedent,
    ]);
}

#[test]
fn test_indent_after_inline_comment() {
    lexer_test("
fn my_func() // comment
    let x = 1
", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
        Token::Indent,
        Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
        Token::Dedent,
    ]);
}

#[test]
fn test_dedent_to_zero_after_empty_line_with_spaces() {
    lexer_test("
fn func()
    let x = 1
   
// The line above has spaces, but is empty of tokens.
// This should dedent correctly.
let y = 2
", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
        Token::Indent,
        Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
        Token::Dedent,
        Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_dedent_to_zero_after_empty_line_with_tabs() {
    lexer_test("
fn func()
\t\tlet x = 1
\t
// This should dedent correctly.
let y = 2
", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
        Token::Indent,
        Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
        Token::Dedent,
        Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_dedent_to_inconsistent_level() {
    lexer_test("
fn func()
    let level1 = 1
      let level2 = 2
   // This dedent is to an invalid level, but we should handle it gracefully.
", vec![
        Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
                Token::Indent, // This is an inconsistent indentation level
                Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
                Token::Dedent,
            Token::Dedent,
    ]);
}

#[test]
fn test_file_starting_with_indentation_and_comment() {
    lexer_test("
    // File starts with an indented comment
let x = 1
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_indentation_rules_after_nested_brackets() {
    lexer_test("
let x = [
    1, // Indentation is ignored here
    {
        'a': 2 // And here
    },
    3
]
// Indentation should apply again here
let y = 1
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::LBracket,
        Token::Int, Token::Comma,
        Token::LBrace, Token::SingleQuotedString, Token::Colon, Token::Int, Token::RBrace, Token::Comma,
        Token::Int,
        Token::RBracket, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
    ]);
}

// TODO: Not sure we need to care about unexpected indentation in the lexer.
// #[test]
// #[should_panic(expected = "Unexpected indentation")]
// fn test_indent_dedent_unexpected() {
//     lexer_test("
//         42
//         'Hello'
// ", vec![]);
// }

// #[test]
// #[should_panic(expected = "Unexpected indentation")]
// fn test_indent_dedent_unexpected_inner() {
//     lexer_test("
// 42
//         'Hello'
//     ", vec![]);
// }

#[test]
fn test_if_statement() {
    lexer_test("
if x
    x = 10
else
    x = 20
    ", vec![
        Token::If, Token::Identifier, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
            Token::Dedent,
        Token::Else, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
            Token::Dedent,
    ]);
}

#[test]
fn test_if_block_else_inline() {
    lexer_test("
if x
    x = 10
else: x = 20
", vec![
        Token::If, Token::Identifier, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
            Token::Dedent,
        Token::Else, Token::Colon, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_if_inline_else_block() {
    lexer_test("
if x: x = 10
else
    x = 20
", vec![
        Token::If, Token::Identifier, Token::Colon, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
        Token::Else, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
            Token::Dedent,
    ]);
}

#[test]
fn test_if_statement_with_assignment() {
    lexer_test("
let y = if x > 10
    10
else
    20
    ", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::If, Token::Identifier, Token::GreaterThan, Token::Int, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Int, Token::ExpressionStatementEnd,
            Token::Dedent,
        Token::Else, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Int, Token::ExpressionStatementEnd,
            Token::Dedent,
    ]);
}

#[test]
fn test_if_statement_inline() {
    lexer_test("
if x: x = 10 else: x = 20
", vec![
        Token::If, Token::Identifier, Token::Colon,
            Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
        Token::Else, Token::Colon,
            Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_if_statement_inline_with_assignment() {
    lexer_test("
let x = 50
let y = if x % 2 == 0: x * x else: x / x
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::If, Token::Identifier, Token::Percent, Token::Int, Token::Equal, Token::Int, Token::Colon,
            Token::Identifier, Token::Star, Token::Identifier, Token::ExpressionStatementEnd,
        Token::Else, Token::Colon,
            Token::Identifier, Token::Slash, Token::Identifier, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_if_with_empty_block() {
    lexer_test("
if x
    // empty then
else
    x = 1
", vec![
        Token::If, Token::Identifier, Token::ExpressionStatementEnd,
        Token::Else, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
            Token::Dedent,
    ]);
}

#[test]
fn test_if_with_empty_block_no_else() {
    lexer_test("
if x
    // TODO
", vec![
        Token::If, Token::Identifier, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_if_with_comment_in_empty_block() {
    lexer_test("
if x
    // This block is empty
let y = 1
", vec![
        Token::If, Token::Identifier, Token::ExpressionStatementEnd,
        Token::Let, Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_if_with_empty_else_block_with_followup() {
    lexer_test("
if x
    x = 1
else
    // empty else
x = 2
", vec![
        Token::If, Token::Identifier, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
            Token::Dedent,
        Token::Else, Token::ExpressionStatementEnd,
        Token::Identifier, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_conditional_expression() {
    lexer_test("
let x = 10 if y > 5 else 20
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::Int,
        Token::If, Token::Identifier, Token::GreaterThan, Token::Int, Token::ExpressionStatementEnd,
        Token::Else, Token::Int, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_conditional_expression_no_else() {
    lexer_test("
let x = 10 if y > 5
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::Int,
        Token::If, Token::Identifier, Token::GreaterThan, Token::Int, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_invalid_characters() {
    lexer_error_test("valid @ invalid", SyntaxErrorKind::InvalidToken);
}

#[test]
fn test_while_loop_nested_empty() {
    lexer_test("
while x > 0
    while y < 5
        // nested body
", vec![
        Token::While, Token::Identifier, Token::GreaterThan, Token::Int, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::While, Token::Identifier, Token::LessThan, Token::Int, Token::ExpressionStatementEnd,
            Token::Dedent
    ]);
}

#[test]
fn test_for_loop_nested_empty() {
    lexer_test("
for i in 1..3
    for c in \"ab\"
        // nested body
", vec![
        Token::For, Token::Identifier, Token::In, Token::Int, Token::Range, Token::Int, Token::ExpressionStatementEnd,
            Token::Indent,
            Token::For, Token::Identifier, Token::In, Token::DoubleQuotedString, Token::ExpressionStatementEnd,
            Token::Dedent
    ]);
}

#[test]
fn test_large_nested_structure() {
    let mut input = String::new();
    let mut expected = Vec::new();
    
    for i in 0..100 {
        input.push_str(&format!("fn level{}()\n", i));
        input.push_str(&"    ".repeat(i + 1));
        expected.extend([Token::Fn, Token::Identifier, Token::LParen, Token::RParen, Token::ExpressionStatementEnd, Token::Indent]);
    }
    
    for _ in 0..100 {
        expected.push(Token::Dedent);
    }
    
    lexer_test(&input, expected);
}

#[test]
fn test_index_member_assignment() {
    lexer_test("
obj['prop'] = 1
", vec![
        Token::Identifier, Token::LBracket, Token::SingleQuotedString, Token::RBracket, Token::Assign, Token::Int, Token::ExpressionStatementEnd,
    ]);
}

#[test]
fn test_namespaced_function_call() {
    lexer_test("
Http::new(url)
", vec![
        Token::Identifier, Token::DoubleColon, Token::Identifier, Token::LParen, Token::Identifier, Token::RParen, Token::ExpressionStatementEnd
    ]);
}

#[test]
fn test_lambda_with_empty_body() {
    lexer_test("
let f = fn()
    // empty body
", vec![
        Token::Let, Token::Identifier, Token::Assign, Token::Fn, Token::LParen, Token::RParen, Token::ExpressionStatementEnd
    ]);
}


fn lexer_test(input: &str, expected: Vec<Token>) {
    let lexer = Lexer::new(input);
    let tokens: Vec<Token> = lexer.map(|result| result.unwrap().0).collect();
    assert_eq!(tokens, expected);
}

fn lexer_error_test(input: &str, expected_kind: SyntaxErrorKind) {
    let lexer = Lexer::new(input);
    let results: Vec<_> = lexer.collect();

    let error = results.iter().find_map(|res| res.as_ref().err().cloned());

    assert!(error.is_some(), "Expected a lexer error, but it succeeded without errors.");
    assert_eq!(error.unwrap().kind, expected_kind, "Lexer produced an error of the wrong kind.");
}