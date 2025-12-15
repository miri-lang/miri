use crate::type_checker::utils::{check_success, check_error};

#[test]
fn test_mutable_variable_assignment() {
    let code = "
var x = 1
x = 2
    ";
    check_success(code);
}

#[test]
fn test_immutable_variable_assignment_error() {
    let code = "
let x = 1
x = 2
    ";
    check_error(code, "Cannot assign to immutable variable");
}

#[test]
fn test_shadowing() {
    let code = "
let x = 1
let x = \"string\" 
    ";
    check_success(code);
}

#[test]
fn test_shadowing_in_nested_scope() {
    let code = "
let x = 1
if true:
    let x = \"string\"
    ";
    check_success(code);
}

#[test]
fn test_immutable_struct_member_assignment_error() {
    let code = "
struct Point
    x int
    y int

let p = Point(1, 2)
p.x = 3
    ";
    check_error(code, "Cannot assign");
}

#[test]
fn test_mutable_struct_member_assignment() {
    let code = "
struct Point
    x int
    y int

var p = Point(1, 2)
p.x = 3
    ";
    check_success(code);
}
