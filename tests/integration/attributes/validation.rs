// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_unknown_attribute_e0112() {
    assert_compiler_error(
        r#"
@nonexistent
enum Status
    Ok
    Error
"#,
        "Unknown attribute: @nonexistent",
    );
}

#[test]
fn test_attribute_on_wrong_target_e0113() {
    assert_compiler_error(
        r#"
@non_exhaustive
fn foo()
    let x = 1
"#,
        "Attribute @non_exhaustive is not valid on a function declaration",
    );
}

#[test]
fn test_attribute_on_class_reports_wrong_target() {
    assert_compiler_error(
        r#"
@must_use
class Widget
    let size = 1
"#,
        "Attribute @must_use is not valid on a class declaration",
    );
}

/// Class members are checked through their class rather than as top-level
/// statements, so their attributes need validating on that path too.
#[test]
fn test_attribute_on_a_class_member_is_validated() {
    assert_compiler_error(
        r#"
class Holder
    @non_exhaustive
    fn size(self) int
        return 1
"#,
        "Attribute @non_exhaustive is not valid on a function declaration",
    );
}

#[test]
fn test_two_attributes_on_one_declaration_are_both_recorded() {
    assert_runs_with_output(
        r#"
@must_use
@non_exhaustive
enum Outcome
    Win
    Lose

let outcome = Outcome.Win
match outcome
    Outcome.Win: println("win")
    Outcome.Lose: println("lose")
"#,
        "win",
    );
}

#[test]
fn test_second_attribute_is_validated_too() {
    assert_compiler_error(
        r#"
@must_use
@nonexistent
enum Outcome
    Win
    Lose
"#,
        "Unknown attribute: @nonexistent",
    );
}

#[test]
fn test_attribute_before_struct_is_rejected() {
    assert_compiler_error(
        r#"
@must_use
struct Point
    x int
    y int
"#,
        "Unsupported Attribute Target",
    );
}

#[test]
fn test_attribute_before_trait_is_rejected() {
    assert_compiler_error(
        r#"
@must_use
trait Drawable
    fn draw(self)
"#,
        "Unsupported Attribute Target",
    );
}

#[test]
fn test_attribute_argument_mismatch_e0114() {
    assert_compiler_error(
        r#"
@must_use("extra")
enum Result
    Ok
    Err
"#,
        "Attribute Argument Extra",
    );
}
