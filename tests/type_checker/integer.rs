// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_integer_literals() {
    check_exprs_type(vec![
        ("1", type_int()),
        ("0", type_int()),
        ("-1", type_int()),
        ("1234567890", type_int()),
    ]);
}

#[test]
fn test_integer_arithmetic_expressions() {
    check_exprs_type(vec![
        ("1 + 2", type_int()),
        ("1 - 2", type_int()),
        ("1 * 2", type_int()),
        ("1 / 2", type_int()),
        ("1 % 2", type_int()),
        ("1 + 2 * 3", type_int()),
        ("(1 + 2) * 3", type_int()),
    ]);
}

#[test]
fn test_integer_unary_expressions() {
    check_exprs_type(vec![
        ("-1", type_int()),
        ("+1", type_int()),
        ("-(1 + 2)", type_int()),
    ]);
}

#[test]
fn test_integer_comparisons() {
    check_exprs_type(vec![
        ("1 < 2", type_bool()),
        ("1 <= 2", type_bool()),
        ("1 > 2", type_bool()),
        ("1 >= 2", type_bool()),
        ("1 == 2", type_bool()),
        ("1 != 2", type_bool()),
    ]);
}

#[test]
fn test_integer_bitwise_operations() {
    check_exprs_type(vec![
        ("1 & 2", type_int()),
        ("1 | 2", type_int()),
        ("1 ^ 2", type_int()),
        ("~1", type_int()),
    ]);
}

#[test]
fn test_valid_integer_arithmetic_variables() {
    check_vars_type(
        "
let x = 1 + 2
let y = x * 3
let z = y / x
let w = z % 2
",
        vec![
            ("x", type_int()),
            ("y", type_int()),
            ("z", type_int()),
            ("w", type_int()),
        ],
    );
}

#[test]
fn test_explicit_integer_type() {
    check_vars_type(
        "
let x int = 1
let y int = -5
",
        vec![("x", type_int()), ("y", type_int())],
    );
}

#[test]
fn test_integer_assignment_operators() {
    check_vars_type(
        "
var x = 1
x += 2
x -= 1
x *= 3
x /= 2
x %= 2
",
        vec![("x", type_int())],
    );
}

#[test]
fn test_explicit_type_mismatch() {
    check_error(
        "
let x int = 1.5
",
        "Type mismatch for variable 'x'",
    );

    check_error(
        "
let x int = true
",
        "Type mismatch for variable 'x'",
    );
}

#[test]
fn test_integer_bool_mismatch() {
    check_error(
        "
let x = 1 + true
",
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_integer_float_mismatch() {
    check_error(
        "
let x = 1 + 1.5
",
        "Type mismatch: int and f32 are not compatible for arithmetic operation",
    );
}

#[test]
fn test_invalid_integer_assignment() {
    check_error(
        "
var x = 1
x = 1.5
",
        "Type mismatch in assignment",
    );
}

#[test]
fn test_invalid_bitwise_operands() {
    check_error(
        "
let x = 1 & 1.5
",
        "Invalid types for bitwise operation",
    );

    check_error(
        "
let x = 1 | true
",
        "Invalid types for bitwise operation",
    );
}

#[test]
fn test_specific_integer_types() {
    // Test assignment of literals to specific types
    // This checks if type_int() (literal) is compatible with specific integer types
    check_vars_type(
        "
let a i8 = 1
let b i16 = 2
let c i32 = 3
let d i64 = 4
let e i128 = 5
let f u8 = 6
let g u16 = 7
let h u32 = 8
let i u64 = 9
let j u128 = 10
",
        vec![
            ("a", type_i8()),
            ("b", type_i16()),
            ("c", type_i32()),
            ("d", type_i64()),
            ("e", type_i128()),
            ("f", type_u8()),
            ("g", type_u16()),
            ("h", type_u32()),
            ("i", type_u64()),
            ("j", type_u128()),
        ],
    );
}

#[test]
fn test_bitwise_on_specific_types() {
    check_vars_type(
        "
let a u8 = 1
let b u8 = 2
let c = a & b
",
        vec![("c", type_u8())],
    );
}

#[test]
fn test_mixed_integer_arithmetic_fail() {
    // Strict typing: i8 + i16 should fail without cast
    check_error(
        "
let x i8 = 1
let y i16 = 2
x + y
",
        "Type mismatch: i8 and i16 are not compatible for arithmetic operation",
    );
}

#[test]
fn test_mixed_integer_bitwise_fail() {
    check_error(
        "
let x u8 = 1
let y u16 = 2
x & y
",
        "Type mismatch: u8 and u16 are not compatible for bitwise operation",
    );
}

#[test]
fn test_unary_on_specific_types() {
    check_vars_type(
        "
let a i8 = 1
let b = -a
let c i16 = 2
let d = -c
",
        vec![("b", type_i8()), ("d", type_i16())],
    );
}

#[test]
fn test_comparison_on_specific_types() {
    check_exprs_type(vec![
        ("let a i8 = 1\nlet b i8 = 2\na < b", type_bool()),
        ("let a u32 = 1\nlet b u32 = 2\na == b", type_bool()),
    ]);
}

#[test]
fn test_comparison_mixed_types_success() {
    check_exprs_type(vec![
        ("let a i8 = 1\nlet b i16 = 2\na < b", type_bool()),
        ("let a u32 = 1\nlet b u64 = 2\na == b", type_bool()),
    ]);
}

#[test]
fn test_assignment_compatibility() {
    // Int literal to specific type is allowed
    check_success("let a i8 = 1");

    // Specific type to Int (variable) - inferred as specific type
    check_vars_type("let a i8 = 1\nlet b = a", vec![("b", type_i8())]);

    // Smaller to larger - allowed
    check_vars_type("let a i8 = 1\nlet b i16 = a", vec![("b", type_i16())]);

    // Larger to smaller - fail
    check_error("let a i16 = 1\nlet b i8 = a", "Type mismatch");
}
