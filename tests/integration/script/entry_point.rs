// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Entry-point selection: which statements run, and what happens when none do.
//!
//! Script-mode wrapping demotes top-level statements into a synthetic `main`.
//! A program that declares its own `main` skips that wrapping, so a top-level
//! statement beside it has nowhere to run. These tests pin the diagnostics that
//! make the three ways a program can end up with nothing to execute visible
//! instead of silent.

use super::utils::*;

#[test]
fn test_top_level_statement_beside_main_is_rejected() {
    let code = r#"
println("never runs")

fn main()
    println("in main")
"#;
    assert_compiler_error(code, "MER_TYP_069");
}

#[test]
fn test_top_level_statement_beside_main_names_the_statement() {
    let code = r#"
println("never runs")

fn main()
    println("in main")
"#;
    assert_compiler_error(code, "top-level statement");
}

#[test]
fn test_top_level_const_beside_main_is_allowed() {
    let code = r#"
const SIZE = 4

fn main()
    println(f"{SIZE}")
"#;
    assert_runs_with_output(code, "4");
}

#[test]
fn test_script_without_main_still_wraps() {
    let code = r#"
println("just script")
"#;
    assert_runs_with_output(code, "just script");
}

#[test]
fn test_declarations_without_main_are_reported_as_nothing_to_run() {
    let code = r#"
fn helper() int
    1
"#;
    assert_build_warning(code, "MER_BLD_021");
}

#[test]
fn test_empty_program_is_reported_as_nothing_to_run() {
    assert_build_warning("\n", "MER_BLD_021");
}

/// The warning must not displace a real error: a module whose imports collide
/// has something to say that matters more than its lack of an entry point.
#[test]
fn test_a_real_error_outranks_the_nothing_to_run_warning() {
    let code = r#"
fn helper() int
    undefined_name
"#;
    assert_build_error(code, "MER_TYP_034");
}

/// A program that does run is never warned about.
#[test]
fn test_a_program_with_main_is_not_warned_about() {
    let code = r#"
fn main()
    println("ran")
"#;
    let result = crate::utils::miri_build(code);
    assert!(
        !result.output().contains("MER_BLD_021"),
        "a program with 'main' was warned that it has nothing to run:\n{}",
        result.output()
    );
}

#[test]
fn test_duplicate_function_declaration_is_rejected() {
    let code = r#"
fn f() int
    1

fn f() int
    2

fn main()
    println(f"{f()}")
"#;
    assert_compiler_error(code, "MER_TYP_070");
}

#[test]
fn test_duplicate_function_declaration_names_the_first_site() {
    let code = r#"
fn f() int
    1

fn f() int
    2

fn main()
    println(f"{f()}")
"#;
    assert_compiler_error(code, "already declared");
}

/// A diagnostic rendered against an empty span points at the file's first
/// token, which is not a location a tool can act on. The parser records no span
/// for `if`, `while`, `for` or `return` statements, so these pin the fallback
/// that keeps the report inside the statement it is about.
#[test]
fn test_the_report_points_inside_a_top_level_if_not_at_the_file_start() {
    let code = r#"fn main()
    println("a")

if true
    println("b")
"#;
    let output = crate::utils::miri_check(code).output();
    assert!(
        output.contains("MER_TYP_069"),
        "expected MER_TYP_069:\n{}",
        output
    );
    assert!(
        !output.contains(":1:1"),
        "the report landed on the file's first token instead of the statement:\n{}",
        output
    );
}

#[test]
fn test_the_report_points_inside_a_top_level_for_not_at_the_file_start() {
    let code = r#"fn main()
    println("a")

for i in 0..1
    println("b")
"#;
    let output = crate::utils::miri_check(code).output();
    assert!(
        output.contains("MER_TYP_069"),
        "expected MER_TYP_069:\n{}",
        output
    );
    assert!(
        !output.contains(":1:1"),
        "the report landed on the file's first token instead of the statement:\n{}",
        output
    );
}

/// The report lands on the later declaration, the one an author removes.
#[test]
fn test_the_duplicate_report_points_at_the_second_declaration() {
    let code = r#"fn f() int
    1

fn f() int
    2

fn main()
    println(f"{f()}")
"#;
    let output = crate::utils::miri_check(code).output();
    assert!(
        output.contains("MER_TYP_070"),
        "expected MER_TYP_070:\n{}",
        output
    );
    assert!(
        output.contains(":4:4"),
        "expected the report on the second declaration's name at 4:4:\n{}",
        output
    );
}

/// A file of `@test` declarations is not a program with nothing to run: the
/// entry point it lacks is the one the test runner supplies. Warning about it
/// fails a correct artifact, and the first branch of that warning's help — add
/// a `main` — is an edit the runner rejects outright.
#[test]
fn test_a_test_file_is_not_warned_that_it_has_nothing_to_run() {
    let code = r#"
use system.testing

@test
fn test_adds()
    assert(1 + 1 == 2)
"#;
    let result = crate::utils::miri_build(code);
    assert!(
        result.success,
        "a file of '@test' declarations must build:\n{}",
        result.output()
    );
    let output = result.output();
    assert!(
        !output.contains("MER_BLD_021"),
        "a test file was warned that it has nothing to run:\n{}",
        output
    );
    assert!(
        !output.contains("warning"),
        "building a test file must be clean of warnings:\n{}",
        output
    );
}

/// Building a test file instead of testing it is still worth saying — the
/// executable runs none of its tests — so the report names the command that
/// does run them.
#[test]
fn test_building_a_test_file_points_at_miri_test() {
    let code = r#"
use system.testing

@test
fn test_adds()
    assert(1 + 1 == 2)
"#;
    let output = crate::utils::miri_build(code).output();
    assert!(
        output.contains("MER_BLD_026"),
        "building a test file must report that it was built rather than tested:\n{}",
        output
    );
    assert!(
        output.contains("miri test"),
        "the report must name the command that runs the tests:\n{}",
        output
    );
    assert!(
        !output.contains("add a 'main' function"),
        "the report must not propose an edit the test runner rejects:\n{}",
        output
    );
}

/// The note is about the missing entry point, not about the attribute: a test
/// file that also declares `main` is a program, and the runner refuses it for
/// that reason rather than the build reporting on it.
#[test]
fn test_a_test_file_that_declares_main_is_not_reported_on() {
    let code = r#"
use system.testing

@test
fn test_adds()
    assert(1 + 1 == 2)

fn main()
    println("ran")
"#;
    let output = crate::utils::miri_build(code).output();
    assert!(
        !output.contains("MER_BLD_026"),
        "a test file declaring 'main' has an entry point and needs no report:\n{}",
        output
    );
    assert!(
        !output.contains("MER_BLD_021"),
        "a program with 'main' was warned that it has nothing to run:\n{}",
        output
    );
}

/// A `const` beside the tests stays a declaration, so the file is still a test
/// file rather than a script with a statement to run.
#[test]
fn test_a_test_file_holding_a_const_is_still_a_test_file() {
    let code = r#"
use system.testing

const LIMIT = 3

@test
fn test_reads_the_const()
    assert(LIMIT == 3)
"#;
    let output = crate::utils::miri_build(code).output();
    assert!(
        output.contains("MER_BLD_026"),
        "a test file declaring a const must report as a test file:\n{}",
        output
    );
    assert!(
        !output.contains("warning"),
        "building a test file must be clean of warnings:\n{}",
        output
    );
}
