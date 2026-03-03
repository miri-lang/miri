// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_runs, assert_runs_many, assert_runs_with_output};

#[test]
fn type_alias_simple() {
    assert_runs("type MyInt is int\nlet x MyInt = 5");
}

#[test]
fn type_alias_string() {
    assert_runs_with_output(
        r#"
use system.io
type ID is String
let id ID = "abc-123"
println(id)
"#,
        "abc-123",
    );
}

#[test]
fn type_alias_in_variable() {
    assert_runs_with_output(
        r#"
use system.io
type MyInt is int
let x MyInt = 42
println(f"{x}")
"#,
        "42",
    );
}

#[test]
fn type_alias_chain() {
    assert_runs_with_output(
        r#"
use system.io
type A is int
type B is A
let x B = 99
println(f"{x}")
"#,
        "99",
    );
}

#[test]
fn type_alias_as_function_parameter_and_return() {
    assert_runs_with_output(
        r#"
use system.io
type MyInt is int

fn double(x MyInt) MyInt
    return x * 2

println(f"{double(21)}")
"#,
        "42",
    );
}

#[test]
fn type_alias_with_list() {
    assert_runs(
        r#"
type IntList is [int]
let nums IntList = [1, 2, 3]
"#,
    );
}

#[test]
fn type_alias_with_map() {
    assert_runs(
        r#"
type StringIntMap is {String: int}
let map StringIntMap = {"a": 1, "b": 2}
"#,
    );
}

#[test]
fn type_alias_with_tuple() {
    assert_runs(
        r#"
type Pair is (int, int)
let p Pair = (1, 2)
"#,
    );
}

#[test]
fn type_alias_with_nullable() {
    assert_runs(
        r#"
type OptionalInt is int?
var x OptionalInt = 5
x = None
"#,
    );
}

#[test]
fn type_alias_in_struct() {
    assert_runs(
        r#"
type MyInt is int

struct Point
    x MyInt
    y MyInt

let p = Point(1, 2)
"#,
    );
}

#[test]
fn type_alias_in_for_loop() {
    assert_runs(
        r#"
type Numbers is [int]
let nums Numbers = [1, 2, 3]
for n in nums
    let x = n * 2
"#,
    );
}

#[test]
fn type_alias_multiple_uses() {
    assert_runs_many(&[
        "type MyInt is int\nlet a MyInt = 1\nlet b MyInt = 2",
        "type MyFloat is float\nlet x MyFloat = 1.5\nlet y MyFloat = 2.5",
        "type MyBool is bool\nlet t MyBool = true\nlet f MyBool = false",
    ]);
}

#[test]
fn type_alias_deeply_nested() {
    assert_runs(
        r#"
type IntList is [int]
type IntListList is [IntList]
let deep IntListList = [[1, 2], [3, 4]]
"#,
    );
}
