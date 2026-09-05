// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Where a diagnostic points.
//!
//! A check that runs over a whole declaration or a whole `match` has to carry
//! its subject's location down to the report. When it does not, the diagnostic
//! arrives with the span every AST node starts with — the empty one — and the
//! renderer underlines the file's first token, which is usually a `use` line
//! with nothing to do with the error. These tests pin the location of the
//! reports that used to land there.

use crate::integration::utils::*;

/// The location the renderer prints for a diagnostic that carries no span.
const FILE_START: &str = ":1:1";

/// `check` output with colour escapes removed, for tests that read locations.
fn check_output(code: &str) -> String {
    crate::utils::strip_ansi(&crate::utils::miri_check(code).output())
}

/// Asserts `code` reports `expected_code` somewhere other than the first token.
fn assert_reports_away_from_file_start(code: &str, expected_code: &str) {
    let output = check_output(code);
    assert!(
        output.contains(expected_code),
        "expected {expected_code}:\n{output}"
    );
    assert!(
        !output.contains(FILE_START),
        "{expected_code} landed on the file's first token instead of the error:\n{output}"
    );
}

/// Asserts `code` reports `expected_code` at `line:column`.
fn assert_reports_at(code: &str, expected_code: &str, line: u32, column: u32) {
    let output = check_output(code);
    assert!(
        output.contains(expected_code),
        "expected {expected_code}:\n{output}"
    );
    let location = format!(":{line}:{column}");
    assert!(
        output.contains(&location),
        "expected {expected_code} at {location}:\n{output}"
    );
}

#[test]
fn test_non_exhaustive_match_points_at_the_subject() {
    let code = r#"enum Color
    Red
    Green
    Blue

fn describe(c Color) String
    match c
        Color.Red: "red"
        Color.Green: "green"

fn main()
    println(describe(Color.Red))
"#;
    // `match c` is line 7; the subject `c` sits at column 11.
    assert_reports_at(code, "MER_TYP_038", 7, 11);
}

#[test]
fn test_non_exhaustive_match_names_the_missing_variants() {
    let code = r#"enum Color
    Red
    Green
    Blue

fn describe(c Color) String
    match c
        Color.Red: "red"
        Color.Green: "green"

fn main()
    println(describe(Color.Red))
"#;
    let output = check_output(code);
    assert!(
        output.contains("Missing variants: Blue"),
        "the report should name the variant left uncovered:\n{output}"
    );
}

#[test]
fn test_non_exhaustive_option_match_points_at_the_subject() {
    let code = r#"fn main()
    let x int? = 5
    match x
        Some(v): println(f"{v}")
"#;
    assert_reports_at(code, "MER_TYP_038", 3, 11);
}

#[test]
fn test_missing_return_points_at_the_signature() {
    let code = r#"use system.io

fn pick(n int) int
    if n > 0
        return 1

fn main()
    println(f"{pick(3)}")
"#;
    assert_reports_at(code, "MER_TYP_054", 3, 1);
}

#[test]
fn test_missing_return_does_not_point_at_a_leading_use_line() {
    let code = r#"use system.io

fn pick(n int) int
    if n > 0
        return 1

fn main()
    println(f"{pick(3)}")
"#;
    assert_reports_away_from_file_start(code, "MER_TYP_054");
}

#[test]
fn test_continue_outside_a_loop_points_at_the_statement() {
    let code = r#"fn main()
    continue
"#;
    assert_reports_at(code, "MER_TYP_050", 2, 5);
}

#[test]
fn test_a_redeclared_type_points_at_the_second_declaration() {
    let code = r#"struct Point
    x int

struct Point
    y int

fn main()
    println("ok")
"#;
    assert_reports_away_from_file_start(code, "MER_TYP_044");
}

#[test]
fn test_a_missing_module_points_at_the_path_that_did_not_resolve() {
    let code = r#"use system.io
use system.missing_module

fn main()
    println("ok")
"#;
    // Column 5 is where `system.missing_module` starts: the path is what the
    // author has to change, not the `use` keyword in front of it.
    assert_reports_at(code, "MER_NAM_002", 2, 5);
}

/// The other member of the `MER_TYP_038` family: a constructor pattern written
/// without its enum prefix. A bare `Err` with no payload is a binding rather
/// than a mistake, so the check only fires on the payload form.
#[test]
fn test_a_bare_result_pattern_points_at_the_pattern() {
    let code = r#"fn classify(r Result<int, String>) int
    match r
        Result.Ok(n): n
        Err(e): 0

fn main()
    println(f"{classify(Result.Ok(1))}")
"#;
    assert_reports_at(code, "MER_TYP_038", 4, 9);
}

#[test]
fn test_each_bare_pattern_in_a_match_is_reported_at_its_own_arm() {
    let code = r#"fn classify(r Result<int, String>) int
    match r
        Ok(n): n
        Err(e): 0

fn main()
    println(f"{classify(Result.Ok(1))}")
"#;
    let output = check_output(code);
    assert!(
        output.contains(":3:9"),
        "the first bare pattern should be reported on its own arm:\n{output}"
    );
    assert!(
        output.contains(":4:9"),
        "the second bare pattern should be reported on its own arm:\n{output}"
    );
}
