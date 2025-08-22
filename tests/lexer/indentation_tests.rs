// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use miri::{lexer::{Token}, syntax_error::SyntaxErrorKind};

use super::utils::*;


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
