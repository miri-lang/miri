// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The flattening the `gpu frame` checker and lowering share.

use miri::ast::gpu_frame_passes::flatten_frame_passes;
use miri::ast::program::Program;
use miri::lexer::Lexer;
use miri::parser::Parser;

fn parse(source: &str) -> Program {
    let mut lexer = Lexer::new(source);
    Parser::new(&mut lexer, source)
        .parse()
        .expect("the source parses")
}

/// The error message flattening reports for `source`, if any.
fn refusal(source: &str) -> Option<String> {
    flatten_frame_passes(&parse(source).body)
        .err()
        .map(|(msg, _)| msg)
}

#[test]
fn passes_are_returned_in_source_order() {
    let program = parse("gpu forall i in 0..4\n    a[i] = 1\ngpu forall i in 0..4\n    b[i] = 2\n");
    let passes = flatten_frame_passes(&program.body).expect("both children are passes");
    assert_eq!(passes.len(), 2);
    assert!(std::ptr::eq(passes[0], &program.body[0]));
    assert!(std::ptr::eq(passes[1], &program.body[1]));
}

#[test]
fn a_repeat_expands_to_one_copy_of_its_passes_per_iteration() {
    let program = parse(concat!(
        "for _ in 2..5\n",
        "    gpu forall i in 0..4\n",
        "        b[i] = a[i]\n",
        "    gpu forall i in 0..4\n",
        "        a[i] = b[i]\n",
    ));
    let passes = flatten_frame_passes(&program.body).expect("the repeat is well formed");
    assert_eq!(passes.len(), 6);
}

#[test]
fn a_child_that_is_not_a_pass_is_refused() {
    let msg = refusal("let x = 1\n").expect("a declaration is not a pass");
    assert!(
        msg.contains("may only contain 'gpu forall' passes"),
        "{msg}"
    );
}

#[test]
fn a_repeat_over_a_non_literal_bound_is_refused() {
    let msg = refusal("for _ in 0..n\n    gpu forall i in 0..4\n        a[i] = 1\n")
        .expect("a variable bound is not a literal");
    assert!(msg.contains("bounds must be integer literals"), "{msg}");
}

#[test]
fn a_descending_repeat_is_refused() {
    let msg = refusal("for _ in 5..2\n    gpu forall i in 0..4\n        a[i] = 1\n")
        .expect("a descending range is refused");
    assert!(msg.contains("non-negative and ascending"), "{msg}");
}
