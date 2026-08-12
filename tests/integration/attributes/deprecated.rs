// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_deprecated_function_compiles_and_warns() {
    assert_compiler_warning(
        r#"
@deprecated("use new_function instead")
fn old_function() int
    42

fn main()
    let x = old_function()
    println(f"{x}")
    "#,
        "use new_function instead",
    );
}

#[test]
fn test_deprecated_function_runs_successfully() {
    assert_runs_with_output(
        r#"
@deprecated("use new_function instead")
fn old_function() int
    42

fn main()
    let x = old_function()
    println(f"{x}")
    "#,
        "42",
    );
}

#[test]
fn test_deprecated_without_argument_is_error() {
    assert_compiler_error(
        r#"
@deprecated
fn old_function() int
    42

fn main()
    println("test")
    "#,
        "Attribute Argument Missing",
    );
}

#[test]
fn test_deprecated_on_class() {
    assert_compiler_error(
        r#"
@deprecated("use NewClass instead")
class OldClass
    var value int

    fn init()
        self.value = 0

fn main()
    let obj = OldClass()
    println(f"{obj.value}")
    "#,
        "not valid on",
    );
}

#[test]
fn test_deprecated_on_enum() {
    assert_compiler_error(
        r#"
@deprecated("use NewStatus instead")
enum OldStatus
    Active
    Inactive

fn main()
    let s = OldStatus.Active
    println("ok")
    "#,
        "not valid on",
    );
}

#[test]
fn test_deprecated_on_wrong_declaration_kind() {
    assert_compiler_error(
        r#"
@deprecated("reason")
var x int = 5

fn main()
    println("test")
    "#,
        "may only",
    );
}
