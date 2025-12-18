use crate::type_checker::utils::{check_error, check_success};

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
    check_success(code);
}

#[test]
fn test_struct_field_type_mismatch() {
    let code = "
struct Point
    x int
    y int

let p = Point(1, true)
    ";
    check_error(code, "Type mismatch");
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
    check_error(code, "has no field");
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
    check_success(code);
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
    check_success(code);
}

#[test]
fn test_enum_associated_value_type_mismatch() {
    let code = "
enum Result
    Ok(int)
    Err(string)

let ok = Result.Ok(\"42\")
    ";
    check_error(code, "Type mismatch");
}
