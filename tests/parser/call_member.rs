// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::parser_error_test;
use miri::error::syntax::SyntaxErrorKind;

#[test]
fn test_call_member_dangling_less_than_reports_missing_expression() {
    // `a <` opens a comparison / generic-argument position that is never closed;
    // the block's `}` arrives where an operand is required, so the parser reports
    // the missing expression rather than panicking.
    parser_error_test(
        "fn foo() { a < }",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "an expression".to_string(),
            found: "}".to_string(),
        },
    );
}
