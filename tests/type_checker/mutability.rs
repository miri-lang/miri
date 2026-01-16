use crate::type_checker::utils::{type_checker_error_test, type_checker_test};

#[test]
fn test_mutable_variable_assignment() {
    let code = "
var x = 1
x = 2
    ";
    type_checker_test(code);
}

#[test]
fn test_immutable_variable_assignment_error() {
    let code = "
let x = 1
x = 2
    ";
    type_checker_error_test(code, "Cannot assign to immutable variable");
}

#[test]
fn test_shadowing() {
    let code = "
let x = 1
let x = \"string\" 
    ";
    type_checker_test(code);
}

#[test]
fn test_shadowing_in_nested_scope() {
    let code = "
let x = 1
if true: let x = \"string\"
    ";
    type_checker_test(code);
}

#[test]
fn test_immutable_struct_member_assignment_error() {
    let code = "
struct Point: x int, y int

let p = Point(1, 2)
p.x = 3
    ";
    type_checker_error_test(code, "Cannot assign to field of immutable variable");
}

#[test]
fn test_mutable_struct_member_assignment() {
    let code = "
struct Point: x int, y int

var p = Point(1, 2)
p.x = 3
    ";
    type_checker_test(code);
}

#[test]
fn test_function_argument_immutability() {
    let code = "
fn foo(x int)
    x = 2
    ";
    type_checker_error_test(code, "Cannot assign to immutable variable");
}

#[test]
fn test_function_argument_shadowing_with_var() {
    let code = "
fn foo(x int)
    var x = 2
    x = 3
    ";
    type_checker_error_test(
        code,
        "Variable 'x' is already defined in this scope. 'var' cannot shadow existing variables.",
    );
}

#[test]
fn test_function_argument_shadowing_with_let() {
    let code = "
fn foo(x int)
    let x = 2
    x = 3
    ";
    type_checker_error_test(code, "Cannot assign to immutable variable");
}

#[test]
fn test_loop_variable_immutability() {
    // Loop variables are immutable by default
    let code = "
for i in 1..10
    i = 5
    ";
    type_checker_error_test(code, "Cannot assign to immutable variable");
}

#[test]
// TODO: Consider not allowing this in the future
fn test_loop_variable_shadowing() {
    let code = "
for i in 1..10
    var i = 5
    i = 6
    ";
    type_checker_test(code);
}

#[test]
fn test_immutable_list_element_assignment() {
    let code = "
let list = [1, 2, 3]
list[0] = 4
    ";
    type_checker_error_test(code, "Cannot assign to element of immutable variable");
}

#[test]
fn test_mutable_list_element_assignment() {
    let code = "
var list = [1, 2, 3]
list[0] = 4
    ";
    type_checker_test(code);
}

#[test]
fn test_immutable_map_value_assignment() {
    let code = "
let map = {\"a\": 1}
map[\"a\"] = 2
    ";
    type_checker_error_test(code, "Cannot assign to element of immutable variable");
}

#[test]
fn test_mutable_map_value_assignment() {
    let code = "
var map = {\"a\": 1}
map[\"a\"] = 2
    ";
    type_checker_test(code);
}

#[test]
fn test_nested_struct_immutability() {
    let code = "
struct Inner: val int
struct Outer: inner Inner

let o = Outer(Inner(1))
o.inner.val = 2
    ";
    type_checker_error_test(code, "Cannot assign to field of immutable variable");
}

#[test]
fn test_nested_struct_mutability() {
    let code = "
struct Inner: val int
struct Outer: inner Inner

var o = Outer(Inner(1))
o.inner.val = 2
    ";
    type_checker_test(code);
}

#[test]
fn test_deeply_nested_immutability() {
    let code = "
struct A: val int
struct B: a A
struct C: b B

let c = C(B(A(1)))
c.b.a.val = 2
    ";
    type_checker_error_test(code, "Cannot assign to field of immutable variable");
}

#[test]
fn test_deeply_nested_mutability() {
    let code = "
struct A: val int
struct B: a A
struct C: b B

var c = C(B(A(1)))
c.b.a.val = 2
    ";
    type_checker_test(code);
}

#[test]
fn test_list_of_structs_immutability() {
    let code = "
struct Point: x int, y int
let list = [Point(1, 2)]
list[0].x = 3
    ";
    type_checker_error_test(code, "Cannot assign to field of immutable variable");
}

#[test]
fn test_list_of_structs_mutability() {
    let code = "
struct Point: x int, y int
var list = [Point(1, 2)]
list[0].x = 3
    ";
    type_checker_test(code);
}

#[test]
fn test_struct_with_list_field_immutability() {
    let code = "
struct Container: items [int]
let c = Container([1, 2])
c.items[0] = 3
    ";
    type_checker_error_test(code, "Cannot assign to element of immutable variable");
}

#[test]
fn test_struct_with_list_field_mutability() {
    let code = "
struct Container: items [int]
var c = Container([1, 2])
c.items[0] = 3
    ";
    type_checker_test(code);
}

#[test]
fn test_lambda_capture_immutability() {
    let code = "
let x = 1
let f = fn()
    x = 2
    ";
    type_checker_error_test(code, "Cannot assign to immutable variable");
}

#[test]
fn test_lambda_capture_mutability() {
    let code = "
var x = 1
let f = fn()
    x = 2
    ";
    type_checker_test(code);
}
