// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_parse_boolean_literal() {
    parse_literal_test("true", boolean(true));
    parse_literal_test("false", boolean(false));
}
