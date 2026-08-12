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
fn test_deprecated_class_warns_at_instantiation() {
    // Constructing a deprecated class is its use site, and the warning names it
    // as a class rather than borrowing the function wording.
    assert_compiler_warning(
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
        "class 'OldClass' is deprecated: use NewClass instead",
    );
}

#[test]
fn test_deprecated_class_still_runs() {
    // Deprecation is advisory: the program keeps compiling and running.
    assert_runs_with_output(
        r#"
@deprecated("use NewClass instead")
class OldClass
    var value int

    fn init()
        self.value = 7

fn main()
    let obj = OldClass()
    println(f"{obj.value}")
    "#,
        "7",
    );
}

#[test]
fn test_deprecated_enum_warns_at_variant_reference() {
    // Naming a variant is the use site of an enum.
    assert_compiler_warning(
        r#"
@deprecated("use NewStatus instead")
enum OldStatus
    Active
    Inactive

fn main()
    let s = OldStatus.Active
    println("ok")
    "#,
        "enum 'OldStatus' is deprecated: use NewStatus instead",
    );
}

#[test]
fn test_deprecated_enum_still_runs() {
    assert_runs_with_output(
        r#"
@deprecated("use NewStatus instead")
enum OldStatus
    Active
    Inactive

fn main()
    let s = OldStatus.Active
    println("ok")
    "#,
        "ok",
    );
}

#[test]
fn test_undeprecated_class_produces_no_warning() {
    // The warning must key off the attribute, not off every instantiation.
    assert_runs_with_output(
        r#"
class Fresh
    var value int

    fn init()
        self.value = 3

fn main()
    let obj = Fresh()
    println(f"{obj.value}")
    "#,
        "3",
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
