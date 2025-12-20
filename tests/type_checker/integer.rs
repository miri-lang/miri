// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::Type;

#[test]
fn test_integer_literals() {
    check_exprs_type(vec![
        ("1", Type::Int),
        ("0", Type::Int),
        ("-1", Type::Int),
        ("1234567890", Type::Int),
    ]);
}

#[test]
fn test_integer_arithmetic_expressions() {
    check_exprs_type(vec![
        ("1 + 2", Type::Int),
        ("1 - 2", Type::Int),
        ("1 * 2", Type::Int),
        ("1 / 2", Type::Int),
        ("1 % 2", Type::Int),
        ("1 + 2 * 3", Type::Int),
        ("(1 + 2) * 3", Type::Int),
    ]);
}

#[test]
fn test_integer_unary_expressions() {
    check_exprs_type(vec![
        ("-1", Type::Int),
        ("+1", Type::Int),
        ("-(1 + 2)", Type::Int),
    ]);
}

#[test]
fn test_integer_comparisons() {
    check_exprs_type(vec![
        ("1 < 2", Type::Boolean),
        ("1 <= 2", Type::Boolean),
        ("1 > 2", Type::Boolean),
        ("1 >= 2", Type::Boolean),
        ("1 == 2", Type::Boolean),
        ("1 != 2", Type::Boolean),
    ]);
}

#[test]
fn test_integer_bitwise_operations() {
    check_exprs_type(vec![
        ("1 & 2", Type::Int),
        ("1 | 2", Type::Int),
        ("1 ^ 2", Type::Int),
        ("~1", Type::Int),
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
            ("x", Type::Int),
            ("y", Type::Int),
            ("z", Type::Int),
            ("w", Type::Int),
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
        vec![("x", Type::Int), ("y", Type::Int)],
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
        vec![("x", Type::Int)],
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
        "Type mismatch: Int and F32 are not compatible for arithmetic operation",
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
    // This checks if Type::Int (literal) is compatible with specific integer types
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
            ("a", Type::I8),
            ("b", Type::I16),
            ("c", Type::I32),
            ("d", Type::I64),
            ("e", Type::I128),
            ("f", Type::U8),
            ("g", Type::U16),
            ("h", Type::U32),
            ("i", Type::U64),
            ("j", Type::U128),
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
        vec![("c", Type::U8)],
    );
}

#[test]
fn test_mixed_integer_arithmetic_fail() {
    // Strict typing: i8 + i16 should fail without cast
    check_error(
        "
let a i8 = 1
let b i16 = 2
let c = a + b
",
        "Type mismatch: I8 and I16 are not compatible for arithmetic operation",
    );
}

#[test]
fn test_mixed_integer_bitwise_fail() {
    check_error(
        "
let a u8 = 1
let b u16 = 2
let c = a & b
",
        "Type mismatch: U8 and U16 are not compatible for bitwise operation",
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
        vec![("b", Type::I8), ("d", Type::I16)],
    );
}

#[test]
fn test_comparison_on_specific_types() {
    check_exprs_type(vec![
        ("let a i8 = 1\nlet b i8 = 2\na < b", Type::Boolean),
        ("let a u32 = 1\nlet b u32 = 2\na == b", Type::Boolean),
    ]);
}

#[test]
fn test_comparison_mixed_types_success() {
    check_exprs_type(vec![
        ("let a i8 = 1\nlet b i16 = 2\na < b", Type::Boolean),
        ("let a u32 = 1\nlet b u64 = 2\na == b", Type::Boolean),
    ]);
}

#[test]
fn test_assignment_compatibility() {
    // Int literal to specific type is allowed
    check_success("let a i8 = 1");

    // Specific type to Int (variable) - inferred as specific type
    check_vars_type("let a i8 = 1\nlet b = a", vec![("b", Type::I8)]);

    // Smaller to larger - allowed
    check_vars_type("let a i8 = 1\nlet b i16 = a", vec![("b", Type::I16)]);

    // Larger to smaller - fail
    check_error("let a i16 = 1\nlet b i8 = a", "Type mismatch");
}
