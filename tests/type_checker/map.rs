// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_map_literal() {
    type_checker_expr_type_test(
        "{ \"a\": 1, \"b\": 2 }",
        type_map(type_string(), type_int()),
    );
}

#[test]
fn test_map_variable_definitions() {
    type_checker_vars_type_test(
        "
        let m1 {int: int} = { 10: 100, 20: 200 } 
        let m2 map<string, float> = { \"a\": 1.1, \"b\": 2.2 }
        let m3 map<i128, f64> = { 1: 1.1, 2: 2.2 }
",
        vec![
            ("m1", type_map(type_int(), type_int())),
            ("m2", type_map(type_string(), type_float())),
            ("m3", type_map(type_i128(), type_f64())),
        ],
    )
}

#[test]
fn test_map_literal_mixed_keys_error() {
    type_checker_error_test("{ \"a\": 1, 2: 2 }", "Map keys must have the same type");
}

#[test]
fn test_map_literal_mixed_values_error() {
    type_checker_error_test(
        "{ \"a\": 1, \"b\": \"s\" }",
        "Map values must have the same type",
    );
}

#[test]
fn test_map_indexing() {
    type_checker_expr_type_test("{ \"a\": 1 }[\"a\"]", type_int());
}

#[test]
fn test_map_indexing_invalid_key_type() {
    type_checker_error_test("{ \"a\": 1 }[1]", "Invalid map key type");
}

#[test]
fn test_map_indexing_variable() {
    type_checker_expr_type_test(
        "
let k = \"a\"
{ \"a\": 1 }[k]
",
        type_int(),
    );
}

#[test]
fn test_map_indexing_function_call() {
    type_checker_expr_type_test(
        "
fn get_key() string
    return \"a\"

{ \"a\": 1 }[get_key()]
",
        type_int(),
    );
}

#[test]
fn test_map_indexing_generic_function_call() {
    type_checker_expr_type_test(
        "
fn get_key<T>(key T) T
    return key

{ \"a\": 1.2 }[get_key<string>(\"a\")]
",
        type_f32(),
    );
}

#[test]
fn test_map_indexing_variable_type_mismatch() {
    type_checker_error_test(
        "
let k = 1
{ \"a\": 1 }[k]
",
        "Invalid map key type",
    );
}

#[test]
fn test_empty_map() {
    type_checker_expr_type_test("{}", type_map(type_void(), type_void()));
}

#[test]
fn test_empty_map_with_specified_types() {
    type_checker_vars_type_test(
        "
    let m1 {string: int} = {}
",
        vec![("m1", type_map(type_string(), type_int()))],
    );
}

#[test]
fn test_empty_map_with_specified_types_named() {
    type_checker_vars_type_test(
        "
    let m2 map<string, float> = {}
",
        vec![("m2", type_map(type_string(), type_float()))],
    );
}

#[test]
fn test_nested_map() {
    type_checker_expr_type_test(
        "{ \"a\": { \"b\": 1 } }",
        type_map(type_string(), type_map(type_string(), type_int())),
    );
}

#[test]
fn test_map_assignment_exact() {
    type_checker_test(
        "
let m {string: int} = { \"a\": 1 }
",
    );
}

#[test]
fn test_map_assignment_mismatch_key() {
    type_checker_error_test(
        "
let m {int: int} = { \"a\": 1 }
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_map_assignment_mismatch_value() {
    type_checker_error_test(
        "
let m {string: string} = { \"a\": 1 }
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_map_mutability() {
    type_checker_test(
        "
var m = { \"a\": 1 }
m[\"a\"] = 2
",
    );
}

#[test]
fn test_map_mutability_type_mismatch() {
    type_checker_error_test(
        "
var m = { \"a\": 1 }
m[\"a\"] = \"s\"
",
        "Type mismatch in assignment",
    );
}

#[test]
fn test_map_assignment_to_immutable_index() {
    type_checker_error_test(
        "
let m = { \"a\": 1 }
m[\"a\"] = 2
",
        "Cannot assign to element of immutable variable",
    );
}

#[test]
fn test_map_of_functions() {
    type_checker_test(
        "
let m = { \"inc\": fn(x int): x + 1, \"dec\": fn(x int): x - 1 }
m[\"inc\"](1)
",
    );
}

#[test]
fn test_map_of_functions_mismatch() {
    type_checker_error_test(
        "
let m = { \"inc\": fn(x int): x + 1, \"dec\": fn(x string): x }
",
        "Map values must have the same type",
    );
}

#[test]
fn test_map_key_types() {
    type_checker_test("{ 1: \"int\", 2: \"int\" }");
    type_checker_test("{ true: \"bool\", false: \"bool\" }");

    // TODO: should this be allowed?
    type_checker_test("{ [1]: \"list\" }");
}
