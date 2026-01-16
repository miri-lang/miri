use crate::type_checker::utils::{type_checker_error_test, type_checker_test};

#[test]
fn test_struct_declaration_and_instantiation() {
    let code = "
struct Point
    x int
    y int

let p = Point(1, 2)
let x = p.x
let y = p.y
    ";
    type_checker_test(code);
}

#[test]
fn test_struct_field_type_mismatch() {
    let code = "
struct Point
    x int
    y int

let p = Point(1, true)
    ";
    type_checker_error_test(code, "Type mismatch");
}

#[test]
fn test_struct_member_access_error() {
    let code = "
struct Point
    x int
    y int

let p = Point(1, 2)
let z = p.z
    ";
    type_checker_error_test(code, "has no field");
}

#[test]
fn test_enum_declaration_and_usage() {
    let code = "
enum Color
    Red
    Green
    Blue

let c = Color.Red
    ";
    type_checker_test(code);
}

#[test]
fn test_enum_with_associated_values() {
    let code = "
enum Result
    Ok(int)
    Err(string)

let ok = Result.Ok(42)
let err = Result.Err(\"error\")
    ";
    type_checker_test(code);
}

#[test]
fn test_enum_associated_value_type_mismatch() {
    let code = "
enum Result
    Ok(int)
    Err(string)

let ok = Result.Ok(\"42\")
    ";
    type_checker_error_test(code, "Type mismatch");
}

#[test]
fn test_struct_instantiation_named_params() {
    let code = "
struct Point
    x int
    y int

let p = Point(x: 1, y: 2)
    ";
    type_checker_test(code);
}

#[test]
fn test_struct_instantiation_named_params_reordered() {
    let code = "
struct Point
    x int
    y int

let p = Point(y: 2, x: 1)
    ";
    type_checker_test(code);
}

#[test]
fn test_struct_instantiation_mixed_params() {
    let code = "
struct Point
    x int
    y int
    z int

let p = Point(1, z: 3, y: 2)
    ";
    type_checker_test(code);
}

#[test]
fn test_struct_instantiation_mixed_params_error() {
    let code = "
struct Point
    x int
    y int

let p = Point(x: 1, 2)
    ";
    type_checker_error_test(code, "Positional arguments cannot follow named arguments");
}

#[test]
fn test_struct_instantiation_missing_field() {
    let code = "
struct Point
    x int
    y int

let p = Point(x: 1)
    ";
    type_checker_error_test(code, "Missing argument for field 'y'");
}

#[test]
fn test_struct_instantiation_unknown_field() {
    let code = "
struct Point
    x int
    y int

let p = Point(x: 1, y: 2, z: 3)
    ";
    type_checker_error_test(code, "Unknown field 'z'");
}

#[test]
fn test_struct_instantiation_duplicate_field() {
    let code = "
struct Point
    x int
    y int

let p = Point(x: 1, x: 2)
    ";
    type_checker_error_test(code, "Duplicate argument 'x'");
}
