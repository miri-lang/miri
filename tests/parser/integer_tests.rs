// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_parse_integer_literal() {
    parse_integer_test("42", int(42));
    parse_integer_test("12345", int(12345));
    parse_integer_test("1_234_567_890", int(1234567890));
    parse_integer_test("9_223_372_036_854_775_807", int(9223372036854775807));

    parse_integer_test("0b1_01_010", int(42));
    parse_integer_test("0xFF", int(255));
    parse_integer_test("0o77", int(63));
    parse_integer_test("0o1234567", int(342391));
}