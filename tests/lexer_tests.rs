use std::vec;

use miri::lexer::{Lexer, Token};

mod shared;


#[test]
fn test_symbols_and_operators() {
    lexer_test(": => -> <- || == != >= <= > < = + - * / % , . ( ) [ ] { }", vec![
        Token::Colon,
        Token::FatArrow,
        Token::Arrow,
        Token::LeftArrow,
        Token::Parallel,
        Token::Eq,
        Token::Neq,
        Token::Gte,
        Token::Lte,
        Token::Gt,
        Token::Lt,
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
fn test_number_edge_cases() {
    lexer_test("0 00 1_000_000 0.0 .5 5. -19 1.0e10 6.67430e-11", vec![
        Token::Int,
        Token::Int,
        Token::Int,
        Token::Float,
        Token::Dot, Token::Int,  // .5 should be parsed as . and 5
        Token::Int, Token::Dot,  // 5. should be parsed as 5 and .,
        Token::Minus, Token::Int, // -19 should be parsed as Minus and 19
        Token::Float, // Scientific notation
        Token::Float, // Scientific notation
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
fn test_symbol_identifier_boundaries() {
    lexer_test(":symbol identifier: :another", vec![
        Token::Symbol,
        Token::Identifier,
        Token::Colon,
        Token::Symbol,
    ]);
}

#[test]
#[should_panic(expected = "Unsupported token")]
fn test_invalid_symbol_syntax() {
    lexer_test("valid ::invalid", vec![]);
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
    lexer_test(shared::code_blocks::USE_STATEMENT, vec![
        Token::Use, Token::Identifier,
        Token::Use, Token::Identifier, Token::Dot, Token::Identifier,
        Token::Use, Token::Identifier, Token::Dot, Token::Identifier, Token::Dot, Token::Identifier,
        Token::Use, Token::Identifier, Token::Comma, Token::Identifier, Token::From, Token::Identifier,
        Token::Use, Token::Identifier, Token::As, Token::Identifier
    ]);
}

#[test]
fn test_inline_comments() {
    lexer_test(shared::code_blocks::INLINE_COMMENTS, vec![
        Token::Var, Token::Identifier, Token::Assign, Token::Int,
        Token::Identifier, Token::SingleQuotedString,
        Token::Use, Token::Identifier, Token::Dot, Token::Identifier,
        Token::Identifier, Token::Assign, Token::Identifier, Token::Plus, Token::Int
    ]);
}

#[test]
fn test_multiline_comments() {
    lexer_test(shared::code_blocks::MULTILINE_COMMENTS, vec![
        Token::Identifier, Token::Assign, Token::DoubleQuotedString,
        Token::Identifier, Token::LParen, Token::RParen, Token::Identifier, Token::Colon,
            Token::Int, Token::Plus, Token::Int,
        Token::Identifier, Token::DoubleQuotedString
    ]);
}

#[test]
fn test_deeply_nested_comments() {
    lexer_test("before /* outer /* inner /* deepest */ inner */ outer */ after", vec![
        Token::Identifier, Token::Identifier
    ]);
}

#[test]
#[should_panic(expected = "Unclosed multiline comment")]
fn test_unclosed_nested_comment() {
    lexer_test("/* outer /* inner */ still open", vec![]);
}

#[test]
fn test_comment_with_code_like_content() {
    lexer_test("/* func(): if else */ real_code", vec![
        Token::Identifier
    ]);
}

#[test]
fn test_declaration() {
    lexer_test(shared::code_blocks::DECLARATION_STATEMENT, vec![
        Token::Identifier, Token::Assign, Token::Int,
        Token::Var, Token::Identifier, Token::Assign, Token::Int,
        Token::Identifier, Token::Identifier, Token::Assign, Token::Int,
        Token::Identifier, Token::Assign, Token::Float,
        Token::Identifier, Token::Identifier, Token::Assign, Token::SingleQuotedString,
        Token::Identifier, Token::Assign, Token::True,
        Token::Identifier, Token::Assign, Token::Int, Token::Percent, Token::Int, Token::Eq, Token::Int,
        Token::Identifier, Token::Assign, Token::Identifier, Token::Lt, Token::Identifier, Token::Comma, Token::Identifier, Token::Gt, Token::LParen, Token::RParen,
        Token::Identifier, Token::Assign, Token::LBracket, Token::Int, Token::Comma, Token::Int, Token::Comma, Token::Int, Token::RBracket,
        Token::Identifier, Token::LBracket, Token::Identifier, Token::RBracket, Token::Assign, Token::LBracket, Token::Float, Token::Comma, Token::Float, Token::Comma, Token::Float, Token::RBracket,
        Token::Identifier, Token::Assign, Token::LBrace, Token::Identifier, Token::Colon, Token::SingleQuotedString, Token::Comma, Token::Identifier, Token::Colon, Token::SingleQuotedString, Token::RBrace,
        Token::Identifier, Token::LBrace, Token::Identifier, Token::Colon, Token::Identifier, Token::RBrace, Token::Assign, Token::LBrace, Token::Identifier, Token::Colon, Token::Int, Token::Comma, Token::Identifier, Token::Colon, Token::Int, Token::RBrace,
    ]);
}

#[test]
fn test_function() {
    lexer_test(shared::code_blocks::FUNCTION_STATEMENT, vec![
        Token::Identifier, Token::LParen, Token::RParen, Token::Colon,
            Token::Indent,
            Token::Identifier, Token::DoubleQuotedString,
            Token::Dedent,
        
        Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::Colon,
            Token::Indent,
            Token::Identifier, Token::Star, Token::Identifier,
            Token::Dedent,

        Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::Comma, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::Colon,
            Token::Indent,
            Token::Identifier, Token::Plus, Token::Identifier,
            Token::Dedent,

        Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::Comma, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::Colon, Token::Identifier, Token::Star, Token::Identifier, 

        Token::Identifier, Token::Assign, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::Colon, Token::Identifier, Token::Star, Token::Identifier,

        Token::Identifier, Token::Assign, Token::LParen, Token::Identifier, Token::Identifier, Token::Comma, Token::Identifier, Token::Identifier, Token::RParen, Token::Colon,
            Token::Indent,
            Token::Identifier, Token::Identifier, Token::Plus, Token::Identifier,
            Token::Identifier, Token::Identifier, Token::Minus, Token::Identifier,
            Token::Dedent,

        Token::Identifier,
        Token::Identifier, Token::Int,
        Token::Identifier, Token::Float, Token::Comma, Token::Float,

        Token::Identifier, Token::LParen, Token::RParen,
        Token::Identifier, Token::LParen, Token::Int, Token::RParen,
        Token::Identifier, Token::LParen, Token::Float, Token::Comma, Token::Float, Token::RParen,

        Token::Identifier, Token::Assign, Token::Identifier, Token::Dot, Token::Identifier, Token::Colon,
            Token::Indent,
            Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::Star, Token::Int,
            Token::Dedent,

        Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::Colon,
            Token::Indent,
            Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::Colon,
                Token::Indent,
                Token::Identifier, Token::Identifier,
                Token::Identifier, Token::Assign, Token::Identifier, Token::Plus, Token::Int,
                Token::For, Token::Identifier, Token::In, Token::Int, Token::Range, Token::Identifier, Token::Colon,
                    Token::Indent,
                    Token::Identifier, Token::Identifier,
                    Token::Dedent,
                Token::Identifier, Token::Identifier,
                Token::Dedent,
            Token::Identifier, Token::LParen, Token::Identifier, Token::RParen,
            Token::Dedent,

        Token::Identifier, Token::LParen, Token::Int, Token::RParen,
    ]);
}

#[test]
fn test_windows_line_endings() {
    lexer_test("line1\r\nline2\r\n", vec![
        Token::Identifier,
        Token::Identifier,
    ]);
}

#[test]
fn test_mixed_whitespace_types() {
    lexer_test("func():\n\t  mixed_indent", vec![
        Token::Identifier, Token::LParen, Token::RParen, Token::Colon,
            Token::Indent,
            Token::Identifier,
            Token::Dedent,
    ]);
}

#[test]
#[should_panic(expected = "Indentation error")]
fn test_uneven_indent_spaces() {
    lexer_test("
// Uneven spaces
func():
   three_spaces():
     two_spaces():
      one_space():
    four_spaces():
      print \"Hello\"
  print \"World\"    
", vec![]);
}

#[test]
#[should_panic(expected = "Indentation error")]
fn test_uneven_indent_tabs() {
    lexer_test("
func():
\ttab():
\t\t\ttab():
\t\ttab():
print \"Hello\"
", vec![]);
}

#[test]
#[should_panic(expected = "Indentation error")]
fn test_uneven_indent_spaces_tabs() {
    lexer_test("
// Mixed tabs and spaces
func():
\t\t\ttab():
\t\t\t print \"Hello\"
  print \"World\"
  \t\t\ttab():
    print \"Indented with tabs\"
  print \"Dedented with spaces\"
", vec![]);
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
            Token::Int, Token::RParen,
    ]);
}

#[test]
fn test_indent_dedent_func_nested() {
    lexer_test("
// Indented call with nested indentation
func(10,
     50,
     nested_func(x int) int:
       print x
       another_func(y int) int:
         print y
         return y + 1
       return x + another_func(1))
", vec![
        Token::Identifier, Token::LParen, Token::Int, Token::Comma,
            Token::Int, Token::Comma,
            Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::Colon,
                Token::Indent,
                Token::Identifier, Token::Identifier,
                Token::Identifier, Token::LParen, Token::Identifier, Token::Identifier, Token::RParen, Token::Identifier, Token::Colon,
                    Token::Indent,
                    Token::Identifier, Token::Identifier,
                    Token::Return, Token::Identifier, Token::Plus, Token::Int,
                    Token::Dedent,
                Token::Return, Token::Identifier, Token::Plus, Token::Identifier, Token::LParen, Token::Int, Token::RParen, Token::RParen,
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
        Token::RParen,
    ]);
}

#[test]
fn test_empty_lines_preserve_indentation_context() {
    lexer_test("
func():
    statement1

    statement2:
        nested
    statement3
", vec![
        Token::Identifier, Token::LParen, Token::RParen, Token::Colon,
            Token::Indent,
            Token::Identifier,
            Token::Identifier, Token::Colon,
                Token::Indent,
                Token::Identifier,
                Token::Dedent,
            Token::Identifier,
            Token::Dedent,
    ]);
}

#[test]
fn test_empty_lines_dont_prevent_dedent() {
    lexer_test("
statement1:
  statement2:
    statement3
  statement4

statement5

", vec![
        Token::Identifier, Token::Colon,
            Token::Indent,
            Token::Identifier, Token::Colon,
                Token::Indent,
                Token::Identifier,
                Token::Dedent,
            Token::Identifier,
            Token::Dedent,

        Token::Identifier,
    ]);
}

#[test]
fn test_multiple_dedent_levels() {
    lexer_test("
func():
    level1():
        level2():
            level3
back_to_root
", vec![
        Token::Identifier, Token::LParen, Token::RParen, Token::Colon,
        Token::Indent,
        Token::Identifier, Token::LParen, Token::RParen, Token::Colon,
        Token::Indent,
        Token::Identifier, Token::LParen, Token::RParen, Token::Colon,
        Token::Indent,
        Token::Identifier,
        Token::Dedent, Token::Dedent, Token::Dedent,
        Token::Identifier,
    ]);
}

#[test]
fn test_indent_dedent_comments() {
    lexer_test(shared::code_blocks::INDENT_DEDENT_COMMENTS, vec![]);
}

#[test]
#[should_panic(expected = "Unexpected indentation")]
fn test_indent_dedent_unexpected() {
    lexer_test("
        42
        'Hello'
    ", vec![]);
}

#[test]
#[should_panic(expected = "Unsupported token")]
fn test_invalid_characters() {
    lexer_test("valid @ invalid", vec![]);
}

#[test]
fn test_large_nested_structure() {
    let mut input = String::new();
    let mut expected = Vec::new();
    
    for i in 0..100 {
        input.push_str(&format!("level{}():\n", i));
        input.push_str(&"    ".repeat(i + 1));
        expected.extend([Token::Identifier, Token::LParen, Token::RParen, Token::Colon, Token::Indent]);
    }
    
    for _ in 0..100 {
        expected.push(Token::Dedent);
    }
    
    lexer_test(&input, expected);
}

fn lexer_test(input: &str, expected: Vec<Token>) {
    let lexer = Lexer::new(input);
    let tokens: Vec<Token> = lexer.map(|(token, _span)| token).collect();
    assert_eq!(tokens, expected);
}

