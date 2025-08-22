// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::syntax_error::SyntaxErrorKind;
use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_implements_statement_single() {
    parse_test("implements ISerializable", vec![
        implements(vec![identifier("ISerializable")])
    ]);
}

#[test]
fn test_implements_statement_multiple() {
    parse_test("implements ISerializable, IClickable, IView", vec![
        implements(vec![
            identifier("ISerializable"),
            identifier("IClickable"),
            identifier("IView")
        ])
    ]);
}

#[test]
fn test_error_implements_trailing_comma() {
    parse_error_test(
        "implements ISerializable,",
        SyntaxErrorKind::UnexpectedToken {
            expected: "identifier".to_string(),
            found: "end of file".to_string(),
        }
    );
}
