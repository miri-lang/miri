// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::ast_builder::*;
use super::utils::*;


#[test]
fn test_parse_float_literal() {
    parse_float_test("3.14", float32(3.14));
    parse_float_test("1.797693134862315", float64(1.797693134862315));

    parse_float_test("1_000.0", float32(1_000.0));
    parse_float_test("1_000_000.123456789", float64(1_000_000.123456789));

    parse_float_test("1.0e10", float32(1.0e10));
    parse_float_test("6.67430e-11", float32(6.67430e-11));
}

#[test]
fn test_parse_float_literal_edge_cases() {
    // Precision edge cases
    parse_float_test("3.141592", float32(3.141592)); // fits f32
    parse_float_test("3.1415927", float32(3.1415927)); // still fits
    parse_float_test("3.14159265", float64(3.14159265)); // too long for f32

    // Largest and smallest values
    parse_float_test("3.4028235e38", float32(3.4028235e38)); // max f32
    parse_float_test("1.17549435e-38", float32(1.17549435e-38)); // min normal f32
    parse_float_test("1.7976931348623157e308", float64(1.7976931348623157e308)); // max f64
    parse_float_test("2.2250738585072014e-308", float64(2.2250738585072014e-308)); // min normal f64

    // Zeros
    parse_float_test("0.0", float32(0.0));
    parse_float_test("0.000000", float32(0.0));

    // Underscore formatting
    parse_float_test("123_456.789", float32(123_456.789));
    parse_float_test("1_000_000.1234567", float64(1_000_000.1234567));
    parse_float_test("1_000_000.12345678", float64(1_000_000.12345678)); // too long

    // Scientific notation variants
    parse_float_test("1.0e+10", float32(1.0e+10));
    parse_float_test("1.0E10", float32(1.0E10));
    parse_float_test("1.0000001e10", float32(1.0000001e10_f32)); // precision edge
    parse_float_test("9.999999e+37", float32(9.999999e37)); // edge of f32

    // Negative exponent
    parse_float_test("1.0e-10", float32(1.0e-10));
    parse_float_test("6.02214076e-23", float64(6.02214076e-23)); // Planck constant

    // Extreme edge underflow
    parse_float_test("1e-46", float64(1e-46)); // below f32 subnormal
    parse_float_test("1e-39", float32(1e-39)); // subnormal but fits
}