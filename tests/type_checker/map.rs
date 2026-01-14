// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_map_literal() {
    check_expr_type(
        "{ \"a\": 1, \"b\": 2 }",
        type_map(type_string(), type_int()),
    );
}

#[test]
fn test_map_variable_definitions() {
    check_vars_type(
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
    check_error("{ \"a\": 1, 2: 2 }", "Map keys must have the same type");
}

#[test]
fn test_map_literal_mixed_values_error() {
    check_error(
        "{ \"a\": 1, \"b\": \"s\" }",
        "Map values must have the same type",
    );
}

#[test]
fn test_map_indexing() {
    check_expr_type("{ \"a\": 1 }[\"a\"]", type_int());
}

#[test]
fn test_map_indexing_invalid_key_type() {
    check_error("{ \"a\": 1 }[1]", "Invalid map key type");
}

#[test]
fn test_map_indexing_variable() {
    check_expr_type(
        "
let k = \"a\"
{ \"a\": 1 }[k]
",
        type_int(),
    );
}

#[test]
fn test_map_indexing_function_call() {
    check_expr_type(
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
    check_expr_type(
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
    check_error(
        "
let k = 1
{ \"a\": 1 }[k]
",
        "Invalid map key type",
    );
}

#[test]
fn test_empty_map() {
    check_expr_type("{}", type_map(type_void(), type_void()));
}

#[test]
fn test_empty_map_with_specified_types() {
    check_vars_type(
        "
    let m1 {string: int} = {}
",
        vec![("m1", type_map(type_string(), type_int()))],
    );
}

#[test]
fn test_empty_map_with_specified_types_named() {
    check_vars_type(
        "
    let m2 map<string, float> = {}
",
        vec![("m2", type_map(type_string(), type_float()))],
    );
}

#[test]
fn test_nested_map() {
    check_expr_type(
        "{ \"a\": { \"b\": 1 } }",
        type_map(type_string(), type_map(type_string(), type_int())),
    );
}

#[test]
fn test_map_assignment_exact() {
    check_success(
        "
let m {string: int} = { \"a\": 1 }
",
    );
}

#[test]
fn test_map_assignment_mismatch_key() {
    check_error(
        "
let m {int: int} = { \"a\": 1 }
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_map_assignment_mismatch_value() {
    check_error(
        "
let m {string: string} = { \"a\": 1 }
",
        "Type mismatch for variable",
    );
}

#[test]
fn test_map_mutability() {
    check_success(
        "
var m = { \"a\": 1 }
m[\"a\"] = 2
",
    );
}

#[test]
fn test_map_mutability_type_mismatch() {
    check_error(
        "
var m = { \"a\": 1 }
m[\"a\"] = \"s\"
",
        "Type mismatch in assignment",
    );
}

#[test]
fn test_map_assignment_to_immutable_index() {
    check_error(
        "
let m = { \"a\": 1 }
m[\"a\"] = 2
",
        "Cannot assign to element of immutable variable",
    );
}

#[test]
fn test_map_of_functions() {
    check_success(
        "
let m = { \"inc\": fn(x int): x + 1, \"dec\": fn(x int): x - 1 }
m[\"inc\"](1)
",
    );
}

#[test]
fn test_map_of_functions_mismatch() {
    check_error(
        "
let m = { \"inc\": fn(x int): x + 1, \"dec\": fn(x string): x }
",
        "Map values must have the same type",
    );
}

#[test]
fn test_map_key_types() {
    check_success("{ 1: \"int\", 2: \"int\" }");
    check_success("{ true: \"bool\", false: \"bool\" }");

    // TODO: should this be allowed?
    check_success("{ [1]: \"list\" }");
}
