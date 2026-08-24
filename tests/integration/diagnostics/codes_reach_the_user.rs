// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Asserts that a real program compiled through the whole pipeline renders the
//! diagnostic code the registry assigns to that failure.
//!
//! The registry's own tests prove codes are unique, parse, and are numbered
//! densely — but they never compile anything, so a code that is wired to the
//! wrong check still passes them. These tests close that gap for the families a
//! user meets most, and they are what caught an integer-literal range error
//! being reported as a range-type mismatch.

use crate::integration::utils::assert_compiler_error;

#[test]
fn integer_literal_too_large_reports_out_of_range() {
    // Regression: this reported `Range Type Mismatch` (the `for x in a..b`
    // family) because "out of range" matched a rule meant for range types.
    assert_compiler_error(
        r#"
fn main()
    let big = 999999999999999999999999999999999999
    println("{big}")
"#,
        "MER_TYP_068",
    );
}

#[test]
fn undefined_name_reports_its_own_code() {
    assert_compiler_error(
        r#"
fn main()
    let x = nonexistent_name
    println("done")
"#,
        "MER_TYP_034",
    );
}

#[test]
fn wrong_argument_count_reports_argument_count_mismatch() {
    assert_compiler_error(
        r#"
fn takes_two(a int, b int) int
    return a + b

fn main()
    let x = takes_two(1)
    println("{x}")
"#,
        "MER_TYP_030",
    );
}

#[test]
fn unknown_field_reports_field_not_found() {
    assert_compiler_error(
        r#"
class Point
    public var x int
    public var y int

fn main()
    let p = Point(1, 2)
    let v = p.z
    println("{v}")
"#,
        "MER_TYP_033",
    );
}

#[test]
fn unknown_enum_variant_reports_invalid_enum_variant() {
    assert_compiler_error(
        r#"
enum Status
    Ok
    Error

fn main()
    let s = Status.Errr
    println("{s}")
"#,
        "MER_TYP_038",
    );
}

#[test]
fn assigning_to_an_immutable_binding_reports_immutability_violation() {
    assert_compiler_error(
        r#"
fn main()
    let x = 1
    x = 2
    println("{x}")
"#,
        "MER_TYP_042",
    );
}
