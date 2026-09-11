// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reporting more than one syntax fault from a single parse.
//!
//! The parser resynchronises at the next line that opens a top-level
//! declaration, so a file whose faults sit in separate declarations reports
//! them all at once instead of one per invocation.

use miri::error::syntax::{SyntaxError, SyntaxErrorKind};
use miri::lexer::Lexer;
use miri::parser::Parser;

/// Every syntax fault one parse reports, in source order.
fn faults(source: &str) -> Vec<SyntaxError> {
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new(&mut lexer, source);
    match parser.parse_all() {
        Ok(_) => Vec::new(),
        Err(errors) => errors.iter().cloned().collect(),
    }
}

/// The source text each fault points at.
fn fault_locations(source: &str, errors: &[SyntaxError]) -> Vec<usize> {
    errors
        .iter()
        .map(|e| source[..e.span.start].lines().count())
        .collect()
}

#[test]
fn three_faults_in_three_functions_are_reported_together() {
    let source = "\
fn alpha() int
    let a: int = 1
    return a

fn beta() int
    let b: int = 2
    return b

fn gamma() int
    let c: int = 3
    return c
";
    let errors = faults(source);

    assert_eq!(
        errors.len(),
        3,
        "one fault per function, from one parse: {errors:?}"
    );
    assert_eq!(fault_locations(source, &errors), vec![2, 6, 10]);
}

#[test]
fn faults_in_a_struct_a_class_and_a_function_are_reported_together() {
    let source = "\
struct Point
    x: int

class Box
    fn get(: int) int
        return 1

fn main()
    let v: int = 1
";
    let errors = faults(source);

    assert_eq!(errors.len(), 3, "one fault per declaration: {errors:?}");
    assert_eq!(fault_locations(source, &errors), vec![2, 5, 9]);
}

#[test]
fn a_declaration_that_cannot_be_parsed_reports_once() {
    let source = "\
fn broken(
    let a = 1
    let b = 2
    let c = 3

fn after()
    let d = 1
";
    let errors = faults(source);

    assert_eq!(
        errors.len(),
        1,
        "the unparseable declaration reports once and the next one is clean: {errors:?}"
    );
}

#[test]
fn a_clean_program_reports_nothing() {
    let source = "\
fn alpha() int
    return 1

fn beta() int
    return 2
";
    assert!(faults(source).is_empty());
}

#[test]
fn the_first_fault_is_the_one_a_single_error_caller_sees() {
    let source = "\
fn alpha() int
    let a: int = 1

fn beta() int
    let b: int = 2
";
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new(&mut lexer, source);
    let error = parser
        .parse()
        .expect_err("the colon annotation must be rejected");

    assert_eq!(error.span, faults(source)[0].span);
}

#[test]
fn a_lexer_fault_per_declaration_is_reported_once_each() {
    let source = "\
fn alpha()
    let a = f\"{\"x\"}\"

fn beta()
    let b = f\"{\"y\"}\"
";
    let errors = faults(source);

    assert_eq!(errors.len(), 2, "both nested-quote faults: {errors:?}");
    for error in &errors {
        assert_eq!(
            error.kind,
            SyntaxErrorKind::InvalidFormattedStringExpression
        );
    }
}

#[test]
fn indented_lines_are_not_mistaken_for_declaration_boundaries() {
    let source = "\
fn alpha()
    let a: int = 1
    fn nested()
        return 2
";
    let errors = faults(source);

    assert_eq!(
        errors.len(),
        1,
        "an indented `fn` belongs to the declaration being abandoned: {errors:?}"
    );
}

#[test]
fn a_declaration_keyword_inside_a_string_is_not_a_boundary() {
    let source = "\
fn alpha()
    let a: int = 1
    let s = \"text
fn beta()
more\"
    println(s)
";
    let errors = faults(source);

    assert_eq!(
        errors.len(),
        1,
        "a multi-line string is one token; resuming inside it would report a \
         fault the file does not have: {errors:?}"
    );
    assert_eq!(fault_locations(source, &errors), vec![2]);
}

#[test]
fn an_unclosed_bracket_does_not_stop_later_declarations_reporting() {
    let source = "\
fn alpha()
    let a: int = 1

fn broken(
    let q = 1

fn gamma()
    let c: int = 2
";
    let errors = faults(source);

    assert_eq!(
        errors.len(),
        3,
        "the bracket the broken declaration left open must not swallow the \
         rest of the file: {errors:?}"
    );
    // The unclosed bracket is reported where the parameter list runs into
    // the next line, not at the `(` itself.
    assert_eq!(fault_locations(source, &errors), vec![2, 5, 8]);
}

#[test]
fn a_fault_inside_a_class_does_not_cascade_through_its_members() {
    let source = "\
class Counter
    fn bump()
        let a: int = 1
    fn reset()
        let b: int = 2

fn main()
    let c: int = 3
";
    let errors = faults(source);

    assert_eq!(
        errors.len(),
        2,
        "the class reports once and the function after it reports once: \
         {errors:?}"
    );
    assert_eq!(fault_locations(source, &errors), vec![3, 8]);
}
