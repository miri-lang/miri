// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_list_literal_int() {
    type_checker_expr_type_test("[1, 2, 3]", type_array(type_int(), 3));
}

#[test]
fn test_list_variable_definitions() {
    type_checker_vars_type_test(
        "
        let l1 = [10, 20, 30]
        let l2 = [\"a\", \"b\", \"c\"]
        let l3 = [1, 2, 3]
        let l4 = [1.1, 2.2, 3.3]
        let l5 = [1.5, 2.5, 3.5]
",
        vec![
            ("l1", type_array(type_int(), 3)),
            ("l2", type_array(type_string(), 3)),
            ("l3", type_array(type_int(), 3)),
            ("l4", type_array(type_f32(), 3)),
            ("l5", type_array(type_f32(), 3)),
        ],
    )
}

#[test]
fn test_list_literal_string() {
    type_checker_expr_type_test("[\"a\", \"b\"]", type_array(type_string(), 2));
}

#[test]
fn test_list_literal_mixed_error() {
    type_checker_error_test("[1, \"a\"]", "Array elements must have the same type");
}

#[test]
fn test_list_indexing() {
    type_checker_expr_type_test("[1, 2, 3][0]", type_int());
}

#[test]
fn test_list_indexing_invalid_index_type() {
    type_checker_error_test("[1, 2, 3][\"a\"]", "Array index must be an integer");
}

#[test]
fn test_list_indexing_on_non_list() {
    type_checker_error_test("1[0]", "Type int is not indexable");
}

#[test]
fn test_list_indexing_variable() {
    type_checker_expr_type_test(
        "
let i = 0
[1, 2, 3][i]
",
        type_int(),
    );
}

#[test]
fn test_list_indexing_function_call() {
    type_checker_expr_type_test(
        "
fn get_index() int
    return 0

[1, 2, 3][get_index()]
",
        type_int(),
    );
}

#[test]
fn test_list_indexing_variable_type_mismatch() {
    type_checker_error_test(
        "
let i = \"0\"
[1, 2, 3][i]
",
        "Array index must be an integer",
    );
}

#[test]
fn test_empty_list() {
    type_checker_expr_type_test("[]", type_array(type_void(), 0));
}

#[test]
fn test_empty_list_with_specified_types() {
    type_checker_error_test(
        "
    let l1 [String] = []
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_empty_list_with_specified_types_named() {
    type_checker_error_test(
        "
    let l2 List<int> = []
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_nested_list() {
    type_checker_expr_type_test("[[1, 2], [3, 4]]", type_array(type_array(type_int(), 2), 2));
}

#[test]
fn test_nested_list_mixed_error() {
    type_checker_error_test("[[1], [\"a\"]]", "Array elements must have the same type");
}

#[test]
fn test_list_assignment_exact() {
    type_checker_test(
        "
let l = [1, 2, 3]
",
    );
}

#[test]
fn test_list_assignment_mismatch_type() {
    type_checker_error_test(
        "
let l [String] = [1, 2, 3]
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_list_assignment_invariant() {
    // Array literals should be inferred based on the target type if possible
    type_checker_test(
        "
let l = [1]
",
    );
}

#[test]
fn test_list_assignment_overflow() {
    type_checker_error_test(
        "
let l [i8] = [1000]
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_list_assignment_signed_unsigned_mismatch() {
    type_checker_error_test(
        "
let l [u8] = [-1]
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_list_assignment_i8_overflow() {
    type_checker_error_test(
        "
let l [i8] = [128]
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_list_mutability() {
    type_checker_test(
        "
var l = [1, 2, 3]
l[0] = 4
",
    );
}

#[test]
fn test_list_mutability_type_mismatch() {
    type_checker_error_test(
        "
var l = [1, 2, 3]
l[0] = \"a\"
",
        "Type mismatch in assignment",
    );
}

#[test]
fn test_list_of_functions() {
    type_checker_test(
        "
let l = [fn(x int): x, fn(x int): x * 2]
l[0](1)
",
    );
}

#[test]
fn test_list_of_functions_mismatch() {
    type_checker_error_test(
        "
let l = [fn(x int): x, fn(x String): x]
",
        "Array elements must have the same type",
    );
}

#[test]
fn test_list_assignment_to_immutable_index() {
    type_checker_error_test(
        "
let l = [1, 2, 3]
l[0] = 4
",
        "Cannot assign to element of immutable variable",
    );
}

#[test]
fn test_array_slicing_with_range() {
    // Array slicing with range literals should type-check successfully
    type_checker_test("[1, 2, 3][0..1]");
    type_checker_test("[1, 2, 3][0..=1]");
}

#[test]
fn test_array_slicing_with_range_variable() {
    type_checker_test(
        "
let r = 0..1
[1, 2, 3][r]
",
    );
}

#[test]
fn test_list_deeply_nested() {
    type_checker_expr_type_test(
        "[[[1, 2], [3, 4]], [[5, 6], [7, 8]]]",
        type_array(type_array(type_array(type_int(), 2), 2), 2),
    );
}

#[test]
fn test_list_many_elements() {
    type_checker_test(
        "
let l = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20]
",
    );
}

#[test]
fn test_list_chained_indexing() {
    type_checker_expr_type_test("[[1, 2], [3, 4]][0][1]", type_int());
}

#[test]
fn test_list_triple_nested_indexing() {
    type_checker_expr_type_test("[[[1]]][0][0][0]", type_int());
}

#[test]
fn test_list_of_tuples() {
    type_checker_expr_type_test(
        "[(1, \"a\"), (2, \"b\"), (3, \"c\")]",
        type_array(type_tuple(vec![type_int(), type_string()]), 3),
    );
}

#[test]
fn test_list_in_function_param() {
    type_checker_error_test(
        "
fn sum(items [int]) int
    return items[0]

sum([1, 2, 3])
",
        "Type mismatch for argument",
    );
}

#[test]
fn test_list_in_function_return() {
    type_checker_error_test(
        "
fn make_list() [int]
    return [1, 2, 3]

make_list()
",
        "Invalid return type",
    );
}

#[test]
fn test_list_complex_expression_elements() {
    type_checker_expr_type_test("[1 + 2, 3 * 4, 5 - 6, 7 / 2]", type_array(type_int(), 4));
}

#[test]
fn test_list_of_nullable() {
    type_checker_error_test(
        "
var items [int?] = [1, 2, 3]
items[0] = None
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_list_iteration_nested() {
    type_checker_test(
        "
let matrix = [[1, 2], [3, 4]]
for row in matrix
    for cell in row
        let x = cell
",
    );
}

#[test]
fn test_list_index_out_of_bounds_literal() {
    type_checker_error_test(
        "
let l = [1, 2, 3]
let x = l[5]
",
        "Array index out of bounds: index 5 but array has 3 elements",
    );
}

#[test]
fn test_list_index_constant_expression_oob() {
    type_checker_error_test(
        "
let l = [1, 2, 3]
let x = l[1 + 5]
",
        "Array index out of bounds",
    );
}

#[test]
fn test_list_of_structs() {
    type_checker_test(
        "
struct Point
    x int
    y int
let points = [Point(1, 1), Point(2, 2)]
let p = points[0]
",
    );
}

#[test]
fn test_list_index_empty_error() {
    type_checker_error_test(
        "
let l = []
let x = l[0]
",
        "Array index out of bounds: index 0 but array has 0 elements",
    );
}
