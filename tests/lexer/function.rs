// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::lexer::Token;

use super::utils::lexer_token_test;

#[test]
fn test_function_with_no_params() {
    lexer_token_test(
        "
// Function with no parameters
fn fancy_print()
  print \"Hello, World!\"
",
        vec![
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::String,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_function_with_params() {
    lexer_token_test(
        "
/* Function with parameters */
fn square(x int) int
  x * x

/* Another function example */
fn add(a int, b int) int
  a + b
",
        vec![
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::Identifier,
            Token::Identifier,
            Token::RParen,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::Star,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Dedent,
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::Identifier,
            Token::Identifier,
            Token::Comma,
            Token::Identifier,
            Token::Identifier,
            Token::RParen,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::Plus,
            Token::Identifier,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_inline_function() {
    lexer_token_test(
        "
// Inline function
fn multiply(a int, b int) int: a * b
",
        vec![
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::Identifier,
            Token::Identifier,
            Token::Comma,
            Token::Identifier,
            Token::Identifier,
            Token::RParen,
            Token::Identifier,
            Token::Colon,
            Token::Identifier,
            Token::Star,
            Token::Identifier,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_lambda_function() {
    lexer_token_test(
        "
// Lambda function
let f = fn (x int) int: x * x
",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Fn,
            Token::LParen,
            Token::Identifier,
            Token::Identifier,
            Token::RParen,
            Token::Identifier,
            Token::Colon,
            Token::Identifier,
            Token::Star,
            Token::Identifier,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_multiline_lambda_function() {
    lexer_token_test(
        "
// Multiline lambda function
let f1 = fn (a float, b float)
  print(a + b)
  print(a - b)
",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Fn,
            Token::LParen,
            Token::Identifier,
            Token::Identifier,
            Token::Comma,
            Token::Identifier,
            Token::Identifier,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::LParen,
            Token::Identifier,
            Token::Plus,
            Token::Identifier,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Identifier,
            Token::LParen,
            Token::Identifier,
            Token::Minus,
            Token::Identifier,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_function_call() {
    lexer_token_test(
        "
// Call with parentheses
fancy_print()
f(10)
f1(5.0, 3.0)
",
        vec![
            Token::Identifier,
            Token::LParen,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Identifier,
            Token::LParen,
            Token::Int,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Identifier,
            Token::LParen,
            Token::Float,
            Token::Comma,
            Token::Float,
            Token::RParen,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_function_call_with_codeblock() {
    lexer_token_test(
        "
// Code block
let y = arr.map(
  fn (x int): x * 2
)
",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Identifier,
            Token::Dot,
            Token::Identifier,
            Token::LParen,
            Token::Fn,
            Token::LParen,
            Token::Identifier,
            Token::Identifier,
            Token::RParen,
            Token::Colon,
            Token::Identifier,
            Token::Star,
            Token::Int,
            Token::RParen,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_namespaced_function_call() {
    lexer_token_test(
        "
Http::new(url)
",
        vec![
            Token::Identifier,
            Token::DoubleColon,
            Token::Identifier,
            Token::LParen,
            Token::Identifier,
            Token::RParen,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_lambda_with_empty_body() {
    lexer_token_test(
        "
let f = fn()
    // empty body
",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Fn,
            Token::LParen,
            Token::RParen,
            Token::ExpressionStatementEnd,
        ],
    );
}

#[test]
fn test_function_modifiers() {
    lexer_token_test(
        "async fn async_task()",
        vec![
            Token::Async,
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
        ],
    );

    lexer_token_test(
        "gpu fn kernel()",
        vec![
            Token::Gpu,
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
        ],
    );

    lexer_token_test(
        "parallel fn parallel_task()",
        vec![
            Token::Parallel,
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
        ],
    );

    // The order of modifiers should not matter to the lexer.
    lexer_token_test(
        "async gpu fn parallel_kernel()",
        vec![
            Token::Async,
            Token::Gpu,
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
        ],
    );
    lexer_token_test(
        "gpu async fn another_kernel()",
        vec![
            Token::Gpu,
            Token::Async,
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::RParen,
        ],
    );
}

#[test]
fn test_function_with_multiline_parameters() {
    // The lexer should not insert Indent/Dedent or ExpressionStatementEnd tokens
    // inside a parameter list that spans multiple lines.
    lexer_token_test(
        "
fn complex_func(
    a int,
    b string,
    c bool, // trailing comma
)
    print(a)
",
        vec![
            Token::Fn,
            Token::Identifier,
            Token::LParen,
            Token::Identifier,
            Token::Identifier,
            Token::Comma,
            Token::Identifier,
            Token::Identifier,
            Token::Comma,
            Token::Identifier,
            Token::Identifier,
            Token::Comma,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Identifier,
            Token::LParen,
            Token::Identifier,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_nested_function_calls() {
    lexer_token_test(
        "a(b(c(1)))",
        vec![
            Token::Identifier,
            Token::LParen,
            Token::Identifier,
            Token::LParen,
            Token::Identifier,
            Token::LParen,
            Token::Int,
            Token::RParen,
            Token::RParen,
            Token::RParen,
        ],
    );
}

#[test]
fn test_lambda_edge_cases() {
    lexer_token_test(
        "let l = fn(): 1",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Fn,
            Token::LParen,
            Token::RParen,
            Token::Colon,
            Token::Int,
        ],
    );

    lexer_token_test(
        "let l = fn()\n  1",
        vec![
            Token::Let,
            Token::Identifier,
            Token::Assign,
            Token::Fn,
            Token::LParen,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Int,
            Token::ExpressionStatementEnd,
            Token::Dedent,
        ],
    );
}
