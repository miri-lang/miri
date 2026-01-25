// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_integer_literals() {
    type_checker_exprs_type_test(vec![
        ("1", type_int()),
        ("0", type_int()),
        ("-1", type_int()),
        ("1234567890", type_int()),
    ]);
}

#[test]
fn test_integer_arithmetic_expressions() {
    type_checker_exprs_type_test(vec![
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
    type_checker_exprs_type_test(vec![
        ("-1", type_int()),
        ("+1", type_int()),
        ("-(1 + 2)", type_int()),
    ]);
}

#[test]
fn test_integer_comparisons() {
    type_checker_exprs_type_test(vec![
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
    type_checker_exprs_type_test(vec![
        ("1 & 2", type_int()),
        ("1 | 2", type_int()),
        ("1 ^ 2", type_int()),
        ("~1", type_int()),
    ]);
}

#[test]
fn test_valid_integer_arithmetic_variables() {
    type_checker_vars_type_test(
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
    type_checker_vars_type_test(
        "
let x int = 1
let y int = -5
",
        vec![("x", type_int()), ("y", type_int())],
    );
}

#[test]
fn test_integer_assignment_operators() {
    type_checker_vars_type_test(
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
    type_checker_error_test(
        "
let x int = 1.5
",
        "Type mismatch for variable 'x'",
    );

    type_checker_error_test(
        "
let x int = true
",
        "Type mismatch for variable 'x'",
    );
}

#[test]
fn test_integer_bool_mismatch() {
    type_checker_error_test(
        "
let x = 1 + true
",
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_integer_float_mismatch() {
    type_checker_error_test(
        "
let x = 1 + 1.5
",
        "Type mismatch: cannot add a float to an integer",
    );
}

#[test]
fn test_invalid_integer_assignment() {
    type_checker_error_test(
        "
var x = 1
x = 1.5
",
        "Type mismatch in assignment",
    );
}

#[test]
fn test_invalid_bitwise_operands() {
    type_checker_error_test(
        "
let x = 1 & 1.5
",
        "Invalid types for bitwise operation",
    );

    type_checker_error_test(
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
    type_checker_vars_type_test(
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
    type_checker_vars_type_test(
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
    type_checker_error_test(
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
    type_checker_error_test(
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
    type_checker_vars_type_test(
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
    type_checker_exprs_type_test(vec![
        ("let a i8 = 1\nlet b i8 = 2\na < b", type_bool()),
        ("let a u32 = 1\nlet b u32 = 2\na == b", type_bool()),
    ]);
}

#[test]
fn test_comparison_mixed_types_success() {
    type_checker_exprs_type_test(vec![
        ("let a i8 = 1\nlet b i16 = 2\na < b", type_bool()),
        ("let a u32 = 1\nlet b u64 = 2\na == b", type_bool()),
    ]);
}

#[test]
fn test_assignment_compatibility() {
    // Int literal to specific type is allowed
    type_checker_test("let a i8 = 1");

    // Specific type to Int (variable) - inferred as specific type
    type_checker_vars_type_test("let a i8 = 1\nlet b = a", vec![("b", type_i8())]);

    // Smaller to larger - allowed
    type_checker_vars_type_test("let a i8 = 1\nlet b i16 = a", vec![("b", type_i16())]);

    // Larger to smaller - fail
    type_checker_error_test("let a i16 = 1\nlet b i8 = a", "Type mismatch");
}

#[test]
fn test_integer_deeply_nested_arithmetic() {
    type_checker_exprs_type_test(vec![
        ("((((1 + 2) * 3) - 4) / 5)", type_int()),
        (
            "1 + (2 + (3 + (4 + (5 + (6 + (7 + (8 + 9)))))))",
            type_int(),
        ),
        ("((((((((1))))))))", type_int()),
    ]);
}

#[test]
fn test_integer_deeply_nested_comparisons() {
    type_checker_exprs_type_test(vec![
        ("(1 < 2) == (3 > 4)", type_bool()),
        (
            "((1 < 2) and (3 > 4)) or ((5 == 6) and (7 != 8))",
            type_bool(),
        ),
    ]);
}

#[test]
fn test_integer_chained_operations() {
    type_checker_exprs_type_test(vec![
        ("1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10", type_int()),
        ("1 * 2 * 3 * 4 * 5 * 6 * 7 * 8 * 9 * 10", type_int()),
        ("1 - 2 - 3 - 4 - 5 - 6 - 7 - 8 - 9 - 10", type_int()),
    ]);
}

#[test]
fn test_integer_mixed_operators_precedence() {
    type_checker_exprs_type_test(vec![
        ("1 + 2 * 3 - 4 / 5 % 6", type_int()),
        ("1 & 2 | 3 ^ 4", type_int()),
        ("-1 + +2 * -3", type_int()),
    ]);
}

#[test]
fn test_integer_compact_formatting() {
    type_checker_exprs_type_test(vec![
        ("1+2", type_int()),
        ("1-2*3", type_int()),
        ("(1+2)*(3-4)", type_int()),
    ]);
}

#[test]
fn test_integer_spaced_formatting() {
    type_checker_exprs_type_test(vec![
        ("1    +    2", type_int()),
        ("(    1    +    2    )", type_int()),
    ]);
}

#[test]
fn test_integer_large_literal() {
    type_checker_test("let x i128 = 170141183460469231731687303715884105727");
}

#[test]
fn test_integer_negative_literal() {
    type_checker_vars_type_test(
        "
let a = -1
let b i8 = -128
let c i16 = -32768
",
        vec![("a", type_int()), ("b", type_i8()), ("c", type_i16())],
    );
}

#[test]
fn test_integer_zero_operations() {
    type_checker_exprs_type_test(vec![
        ("0 + 0", type_int()),
        ("0 * 1000000", type_int()),
        ("0 - 0", type_int()),
        ("0 == 0", type_bool()),
    ]);
}

#[test]
fn test_integer_many_variables_chain() {
    type_checker_test(
        "
let a = 1
let b = a + 1
let c = b + 1
let d = c + 1
let e = d + 1
let f = e + 1
let g = f + 1
let h = g + 1
let i = h + 1
let j = i + 1
",
    );
}

#[test]
fn test_integer_bitwise_combinations() {
    type_checker_exprs_type_test(vec![
        ("(1 & 2) | (3 ^ 4)", type_int()),
        ("~(1 & 2)", type_int()),
        ("~~1", type_int()),
    ]);
}

#[test]
fn test_integer_invalid_types_in_expression() {
    type_checker_error_test("1 + \"string\"", "Invalid types for arithmetic operation");
    type_checker_error_test("1 * [1, 2, 3]", "Invalid types for arithmetic operation");
    type_checker_error_test("1 / {1, 2}", "Invalid types for arithmetic operation");
}
