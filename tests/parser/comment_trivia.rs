// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Comments are buffered by the lexer and claimed by the parser, so a
//! statement carries the comments written around it. The token stream the
//! parser reads is unchanged, so these tests describe attachment only.

use miri::ast::Program;
use miri::lexer::Lexer;
use miri::parser::Parser;

/// Parse without normalizing, so a top-level statement stays where it was
/// written rather than being rehoused by the shell pass.
fn parse(source: &str) -> Program {
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new(&mut lexer, source);
    match parser.parse() {
        Ok(program) => program,
        Err(error) => panic!("the fixture parses: {error:?}"),
    }
}

/// The text of every comment leading `index`, in source order.
fn leading(program: &Program, index: usize) -> Vec<&str> {
    program.body[index]
        .trivia
        .leading_comments
        .iter()
        .map(|comment| comment.text.as_str())
        .collect()
}

/// The text of the comment trailing `index`, if it has one.
fn trailing(program: &Program, index: usize) -> Option<&str> {
    program.body[index]
        .trivia
        .trailing_comment
        .as_ref()
        .map(|comment| comment.text.as_str())
}

#[test]
fn test_a_comment_on_its_own_line_leads_the_statement_below_it() {
    let program = parse("// explain the value\nlet x = 1\n");

    assert_eq!(leading(&program, 0), vec!["// explain the value"]);
    assert_eq!(trailing(&program, 0), None);
}

#[test]
fn test_a_comment_after_code_trails_the_statement_it_follows() {
    let program = parse("let x = 1 // why one\n");

    assert_eq!(trailing(&program, 0), Some("// why one"));
    assert!(
        leading(&program, 0).is_empty(),
        "a comment after code does not also lead its own statement"
    );
}

#[test]
fn test_a_comment_between_two_statements_leads_the_second() {
    let program = parse("let a = 1\n// about b\nlet b = 2\n");

    assert!(
        leading(&program, 0).is_empty(),
        "the first statement has nothing written above it"
    );
    assert_eq!(
        trailing(&program, 0),
        None,
        "the comment is on its own line"
    );
    assert_eq!(leading(&program, 1), vec!["// about b"]);
}

#[test]
fn test_a_run_of_comment_lines_all_lead_the_same_statement() {
    let program = parse("// first\n// second\nlet x = 1\n");

    assert_eq!(leading(&program, 0), vec!["// first", "// second"]);
}

#[test]
fn test_a_statement_carries_both_a_leading_and_a_trailing_comment() {
    let program = parse("// above\nlet x = 1 // beside\n");

    assert_eq!(leading(&program, 0), vec!["// above"]);
    assert_eq!(trailing(&program, 0), Some("// beside"));
}

#[test]
fn test_a_comment_inside_a_function_body_attaches_to_the_statement_it_precedes() {
    let program = parse("fn helper() int\n    // the answer\n    return 1\n");

    assert!(
        leading(&program, 0).is_empty(),
        "the comment belongs to the return, not to the function"
    );
}

#[test]
fn test_a_statement_with_no_comments_carries_none() {
    let program = parse("let x = 1\n");

    assert!(leading(&program, 0).is_empty());
    assert_eq!(trailing(&program, 0), None);
}

#[test]
fn test_a_comment_above_a_statement_in_a_body_leads_that_statement() {
    use miri::ast::statement::StatementKind;

    let program = parse("fn helper() int\n    // the answer\n    return 1\n");

    let StatementKind::FunctionDeclaration(declaration) = &program.body[0].node else {
        panic!("the fixture declares a function");
    };
    let body = declaration.body.as_ref().expect("the function has a body");
    let StatementKind::Block(statements) = &body.node else {
        panic!("the body is a block, found {:?}", body.node);
    };

    let texts: Vec<&str> = statements[0]
        .trivia
        .leading_comments
        .iter()
        .map(|comment| comment.text.as_str())
        .collect();
    assert_eq!(texts, vec!["// the answer"]);
}
