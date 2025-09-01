// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::syntax_error::SyntaxErrorKind;
use super::utils::*;


#[test]
fn test_parse_mismatched_parentheses() {
    // Mismatched brackets should be a syntax error.
    parser_error_test(
        "(5 + 2]", 
        &SyntaxErrorKind::UnexpectedToken { 
            expected: ")".into(),
            found: "]".into() 
        }
    );
}