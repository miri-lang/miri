// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Structural queries over a parsed program.

use miri::ast::analysis::program_uses_regex;
use miri::ast::program::Program;
use miri::lexer::Lexer;
use miri::parser::Parser;

fn parse(source: &str) -> Program {
    let mut lexer = Lexer::new(source);
    Parser::new(&mut lexer, source)
        .parse()
        .expect("the source parses")
}

fn uses_regex(source: &str) -> bool {
    program_uses_regex(&parse(source).body)
}

#[test]
fn test_a_program_without_regex_syntax_uses_none() {
    assert!(!uses_regex(
        "fn main()\n    let text = \"re\"\n    match text\n        \"a\": 1\n        _: 2\n"
    ));
}

#[test]
fn test_a_regex_literal_in_a_function_body_is_found() {
    assert!(uses_regex("fn main()\n    let r = re\"[0-9]+\"\n"));
}

#[test]
fn test_a_regex_literal_nested_in_a_call_in_a_lambda_is_found() {
    assert!(uses_regex(
        "fn main()\n    let f = fn(s String) bool: test(re\"x\", s)\n"
    ));
}

#[test]
fn test_a_regex_literal_in_a_class_method_is_found() {
    assert!(uses_regex(
        "class Finder\n    fn find(s String) bool\n        return re\"[a-z]\".matches(s)\n"
    ));
}

#[test]
fn test_a_regex_match_pattern_is_found() {
    assert!(uses_regex(
        "fn main()\n    match \"ab\"\n        re\"[a-z]+\": 1\n        _: 2\n"
    ));
}

#[test]
fn test_a_regex_literal_in_a_module_binding_is_found() {
    assert!(uses_regex("let digits = re\"[0-9]\"\n"));
}
