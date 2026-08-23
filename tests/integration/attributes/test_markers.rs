// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_test_attribute_compiles_with_no_params_and_no_return() {
    assert_runs(
        r#"
@test
fn simple_test()
    println("test ran")

// Test markers don't execute automatically; they're just markers
let x = 1
"#,
    );
}

#[test]
fn test_test_attribute_with_parameter_fails_e0116() {
    assert_compiler_error(
        r#"
@test
fn bad_test(x int)
    let y = x
"#,
        "Invalid test function signature",
    );
}

#[test]
fn test_test_attribute_with_return_type_fails_e0116() {
    assert_compiler_error(
        r#"
@test
fn bad_test() int
    return 42
"#,
        "Invalid test function signature",
    );
}

#[test]
fn test_test_attribute_on_enum_still_fails_e0113() {
    assert_compiler_error(
        r#"
@test
enum Status
    Ok
    Error
"#,
        "Attribute @test is not valid on an enum declaration",
    );
}

#[test]
fn test_test_attribute_with_argument_fails_e0114() {
    assert_compiler_error(
        r#"
@test("reason")
fn bad_test()
    println("test")
"#,
        "Attribute Argument Extra",
    );
}

#[test]
fn test_ignore_without_test_fails_e0117() {
    assert_compiler_error(
        r#"
@ignore("flaky")
fn not_a_test()
    println("not a test")
"#,
        "requires @test",
    );
}

#[test]
fn test_xfail_without_test_fails_e0117() {
    assert_compiler_error(
        r#"
@xfail("known bug")
fn not_a_test()
    println("not a test")
"#,
        "requires @test",
    );
}

#[test]
fn test_test_with_ignore_compiles() {
    assert_runs_with_output(
        r#"
@test
@ignore("flaky test")
fn ignored_test()
    println("should not run in test mode")

let x = 42
"#,
        "",
    );
}

#[test]
fn test_test_with_xfail_compiles() {
    assert_runs_with_output(
        r#"
@test
@xfail("known bug")
fn expected_to_fail()
    println("expected failure")

let x = 1
"#,
        "",
    );
}

#[test]
fn test_ignore_without_argument_alongside_test_fails_e0114() {
    assert_compiler_error(
        r#"
@test
@ignore
fn bad_test()
    println("test")
"#,
        "Attribute Argument Missing",
    );
}

#[test]
fn test_xfail_without_argument_alongside_test_fails_e0114() {
    assert_compiler_error(
        r#"
@test
@xfail
fn bad_test()
    println("test")
"#,
        "Attribute Argument Missing",
    );
}

#[test]
fn test_test_attribute_on_class_method_checks_signature() {
    assert_compiler_error(
        r#"
class Widget
    @test
    fn bad_method(self, x int)
        let y = x
"#,
        "Invalid test function signature",
    );
}

#[test]
fn test_test_attribute_on_class_method_void_compiles() {
    assert_runs(
        r#"
class Widget
    @test
    fn test_method(self)
        println("method test")

let w = Widget()
"#,
    );
}

#[test]
fn test_test_attribute_invalid_signature_points_at_attribute() {
    use crate::utils::miri_check;

    let code = r#"
// Line 2: some setup
var setup = 1

@test
fn bad_test(x int)
    let y = x
"#;

    let result = miri_check(code);
    let output = result.output();

    if !output.contains("Invalid test function signature") {
        panic!(
            "Expected error 'Invalid test function signature', got:\n{}",
            output
        );
    }

    if !output.contains(":5:") {
        panic!(
            "Expected diagnostic to point at line 5 (the @test attribute), but got:\n{}",
            output
        );
    }
}
