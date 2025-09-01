// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_extends_statement() {
    parser_test("extends BaseClass", vec![
        extends(identifier("BaseClass"))
    ]);
}

#[test]
fn test_extends_statement_namespaced() {
    // This is not allowed, because you can't define a class within a class.
    parser_error_test(
        "extends Core::Base",
        &SyntaxErrorKind::InvalidInheritanceIdentifier
    );
}

#[test]
fn test_error_extends_multiple_classes() {
    // `extends` only supports single inheritance.
    parser_error_test(
        "extends Base, Other",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an end of statement".to_string(),
            found: ",".to_string(),
        }
    );
}

#[test]
fn test_error_extends_with_literal() {
    parser_error_test(
        "extends 123",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "int".to_string(),
        }
    );
}
