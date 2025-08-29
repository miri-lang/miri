// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_forever_loop() {
    parse_test("
forever
    x
", vec![
        forever_statement(
            block(vec![
                expression_statement(identifier("x".into()))
            ])
        )
    ]);
}

#[test]
fn test_forever_loop_with_comment() {
    parse_test("
forever // This is an infinite loop
    x
", vec![
        forever_statement(
            block(vec![
                expression_statement(identifier("x".into()))
            ])
        )
    ]);
}

#[test]
fn test_forever_loop_with_empty_body_and_comment() {
    parse_test("
forever
    // This is an infinite loop
", vec![
        forever_statement(
            empty_statement()
        )
    ]);
}

#[test]
fn test_forever_loop_nested_with_empty_body_and_comment() {
    parse_test("
forever
    forever
        // This is an infinite loop
", vec![
        forever_statement(
            block(vec![
                forever_statement(
                    empty_statement()
                )
            ])
        )
    ]);
}

#[test]
fn test_forever_loop_inline() {
    parse_test("
forever: x
", vec![
        forever_statement(
            expression_statement(identifier("x".into()))
        )
    ]);
}

#[test]
fn test_forever_loop_inline_with_empty_body_and_comment() {
    parse_test("
forever: // This is an infinite loop
", vec![
        forever_statement(
            empty_statement()
        )
    ]);
}

#[test]
fn test_forever_loop_inline_nested_with_empty_body_and_comment() {
    parse_test("
forever: forever: // This is an infinite loop
", vec![
        forever_statement(
            forever_statement(
                empty_statement()
            )
        )
    ]);
}
