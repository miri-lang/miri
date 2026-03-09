use super::utils::{
    type_checker_error_test, type_checker_error_with_help_test, type_checker_test,
    type_checker_vars_type_test,
};
use miri::ast::factory::{type_array, type_f32, type_int, type_string, type_tuple};

#[test]
fn test_array_variable_definitions() {
    type_checker_vars_type_test(
        "
        let a1 [int; 3] = [10, 20, 30]
        let a2 = [\"a\", \"b\", \"c\"]
        let a3 = [1, 2, 3]
        let a4 = [1.1, 2.2, 3.3]
",
        vec![
            ("a1", type_array(type_int(), 3)),
            ("a2", type_array(type_string(), 3)),
            ("a3", type_array(type_int(), 3)),
            ("a4", type_array(type_f32(), 3)),
        ],
    )
}

#[test]
fn test_array_missing_import_suggestion() {
    type_checker_error_with_help_test(
        "
        let items = []
        for i in 0..10
            items.push(i)
        ",
        "Type 'Array(void, 0)' does not have members",
        "Consider importing 'system.collections.array' to use Array methods",
    );
}

#[test]
fn test_heterogeneous_array_elements() {
    type_checker_error_test(
        "let a = [1, \"2\", 3]",
        "Array elements must have the same type",
    );
}

#[test]
fn test_array_initialization_size_mismatch() {
    type_checker_error_test("let a [int; 2] = [1, 2, 3]", "Type mismatch");
}

#[test]
fn test_nested_array_size_compatibility() {
    type_checker_error_test(
        "let a = [[1, 2], [1, 2, 3]]",
        "Array elements must have the same type",
    );
}

#[test]
fn test_multi_dimensional_array_indexing() {
    type_checker_test(
        "
        let a = [[1, 2], [3, 4]]
        let x = a[0][1]
        ",
    );
}

#[test]
fn test_array_index_out_of_bounds_literal() {
    type_checker_error_test(
        "
        let a = [1, 2, 3]
        let x = a[5]
        ",
        "Array index out of bounds: index 5 but array has 3 elements",
    );
}

#[test]
fn test_array_index_boundary_oob() {
    type_checker_error_test(
        "
        let a = [1, 2, 3]
        let x = a[3]
        ",
        "Array index out of bounds: index 3 but array has 3 elements",
    );
}

#[test]
fn test_array_negative_index() {
    type_checker_error_test(
        "
        let a = [1, 2, 3]
        let x = a[-1]
        ",
        "Array index must be a non-negative integer",
    );
}

#[test]
fn test_multi_dimensional_oob() {
    type_checker_error_test(
        "
        let a = [[1, 2], [3, 4]]
        let x = a[0][2]
        ",
        "Array index out of bounds: index 2 but array has 2 elements",
    );
}

#[test]
fn test_nested_oob() {
    type_checker_error_test(
        "
        let a = [[1, 2], [3, 4]]
        let x = a[5][0]
        ",
        "Array index out of bounds: index 5 but array has 2 elements",
    );
}

#[test]
fn test_array_large_index_oob() {
    type_checker_error_test(
        "
        let a = [1, 2, 3]
        let x = a[9223372036854775807]
        ",
        "Array index out of bounds",
    );
}

#[test]
fn test_array_constant_expression_index_oob() {
    type_checker_error_test(
        "
        let a = [1, 2, 3]
        let x = a[1 + 2]
        ",
        "Array index out of bounds",
    );
}

#[test]
fn test_array_declared_constant_expression_index_oob() {
    type_checker_error_test(
        "
        const IDX = 5
        let a = [1, 2, 3]
        let x = a[IDX]
        ",
        "Array index out of bounds",
    );
}

#[test]
fn test_array_non_integer_index() {
    type_checker_error_test(
        "
        let a = [1, 2, 3]
        let x = a[\"0\"]
        ",
        "Array index must be an integer",
    );
}

#[test]
fn test_array_slicing() {
    type_checker_vars_type_test(
        "
        let a = [1, 2, 3]
        let s1 = a[1..2]
        let s2 = a[1..=2]
",
        vec![
            ("a", type_array(type_int(), 3)),
            ("s1", type_array(type_int(), 1)),
            ("s2", type_array(type_int(), 2)),
        ],
    );
}

#[test]
fn test_array_slicing_start_oob() {
    type_checker_error_test(
        "
        let a = [1, 2, 3]
        let x = a[5..6]
        ",
        "Slice start index out of bounds: index 5 but array has 3 elements",
    );
}

#[test]
fn test_array_slicing_end_oob() {
    type_checker_error_test(
        "
        let a = [1, 2, 3]
        let x = a[1..5]
        ",
        "Slice end index out of bounds: index 5 but array has 3 elements",
    );
}

#[test]
fn test_array_slicing_start_greater_than_end() {
    type_checker_error_test(
        "
        let a = [1, 2, 3]
        let x = a[2..1]
        ",
        "Slice start index (2) is greater than end index (1)",
    );
}

#[test]
fn test_array_slicing_negative_index() {
    type_checker_error_test(
        "
        let a = [1, 2, 3]
        let x = a[-1..2]
        ",
        "Slice start index must be a non-negative integer",
    );
}

#[test]
fn test_array_of_structs() {
    type_checker_test(
        "struct Point
    x int
    y int

let points = [Point(1, 1), Point(2, 2)]
let p = points[0]
let x = p.x",
    );
}

#[test]
fn test_array_constant_size_expression() {
    // Constant expressions in array sizes should be folded (e.g., `1 + 2` → `3`)
    type_checker_vars_type_test(
        "let a [int; 1 + 2] = [1, 2, 3]",
        vec![("a", type_array(type_int(), 3))],
    );
}

#[test]
fn test_array_mutability() {
    type_checker_error_test(
        "
        let a = [1, 2, 3]
        a[0] = 10
        ",
        "Cannot assign to element of immutable variable",
    );
}

#[test]
fn test_array_float_index() {
    type_checker_error_test(
        "
        let a = [1, 2, 3]
        let x = a[1.0]
        ",
        "Array index must be an integer",
    );
}

#[test]
fn test_index_empty_array() {
    type_checker_error_test(
        "
        let a = []
        let x = a[0]
        ",
        "Array index out of bounds: index 0 but array has 0 elements",
    );
}

#[test]
fn test_array_indexing_variable() {
    type_checker_test(
        "
let a = [1, 2, 3]
let i = 0
let x = a[i]
",
    );
}

#[test]
fn test_array_indexing_function_call() {
    type_checker_test(
        "
fn get_index() int
    return 0
let a = [1, 2, 3]
let x = a[get_index()]
",
    );
}

#[test]
fn test_array_of_functions() {
    type_checker_test(
        "
let l = [fn(x int): x, fn(x int): x * 2]
l[0](1)
",
    );
}

#[test]
fn test_array_of_tuples() {
    type_checker_vars_type_test(
        "let a = [(1, \"a\"), (2, \"b\")]",
        vec![(
            "a",
            type_array(type_tuple(vec![type_int(), type_string()]), 2),
        )],
    );
}

#[test]
fn test_array_complex_expression_elements() {
    type_checker_vars_type_test(
        "let a = [1 + 2, 3 * 4]",
        vec![("a", type_array(type_int(), 2))],
    );
}

#[test]
fn test_array_iteration() {
    type_checker_test(
        "
let a = [1, 2, 3]
for x in a
    let y = x
",
    );
}
