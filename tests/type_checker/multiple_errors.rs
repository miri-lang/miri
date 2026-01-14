// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::type_checker::utils::check_errors;

#[test]
fn test_multiple_errors() {
    let source = "
let x int = \"string\"
let y bool = 123
";

    check_errors(
        source,
        vec![
            "Type mismatch for variable 'x': expected int, got string",
            "Type mismatch for variable 'y': expected boolean, got int",
        ],
    );
}

#[test]
fn test_function_arguments_multiple_errors() {
    let source = "
fn foo(a int, b float): a + b
foo(\"wrong\", 1)
";
    check_errors(
        source,
        vec![
            "Type mismatch for argument 'a': expected int, got string",
            "Type mismatch for argument 'b': expected float, got int",
        ],
    );
}

#[test]
fn test_list_literal_multiple_errors() {
    let source = "
let l = [1, \"str\", true]
";
    check_errors(
        source,
        vec![
            "List elements must have the same type", // 1 vs "str"
            "List elements must have the same type", // 1 vs true
        ],
    );
}

#[test]
fn test_map_literal_multiple_errors() {
    let source = "
let m = {1: \"a\", \"2\": \"b\", 3: 3}
";
    check_errors(
        source,
        vec![
            "Map keys must have the same type",   // 1 vs "2"
            "Map values must have the same type", // "a" vs 3
        ],
    );
}

#[test]
fn test_match_expression_multiple_errors() {
    let source = "
let x = 1
match x
    1: \"ok\"
    2: 123
    3: true
";
    check_errors(
        source,
        vec![
            "Match branch types mismatch: expected string, got int",
            "Match branch types mismatch: expected string, got boolean",
        ],
    );
}

#[test]
fn test_struct_initialization_multiple_errors() {
    let source = "
struct Point: x int, y int
let p = Point(true, \"str\")
";
    check_errors(
        source,
        vec![
            "Type mismatch for field 'x': expected int, got boolean",
            "Type mismatch for field 'y': expected int, got string",
        ],
    );
}

#[test]
fn test_cascading_error_suppression() {
    let source = "
let x int = unknown_var
";
    // Should only report "Undefined variable: unknown_var"
    // Should NOT report "Type mismatch: expected Int, got Error"
    check_errors(source, vec!["Undefined variable: unknown_var"]);
}
