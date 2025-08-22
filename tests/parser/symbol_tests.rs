// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_parse_string_literal() {
    parse_literal_test("'hello single quote'", string("hello single quote"));
    parse_literal_test("\"hello double quote\"", string("hello double quote"));
}