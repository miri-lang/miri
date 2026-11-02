// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

/// Criterion 1: Basic regex literal with matches() method
#[test]
fn test_regex_literal_basic_match() {
    assert_runs_with_output(
        r#"
use system.text

let r = re"^\d+$"
if r.matches("12345")
    println("true")
else
    println("false")
"#,
        "true",
    );
}

#[test]
fn test_regex_literal_basic_no_match() {
    assert_runs_with_output(
        r#"
use system.text

let r = re"^\d+$"
if r.matches("abc")
    println("true")
else
    println("false")
"#,
        "false",
    );
}

/// Criterion 2: Case-insensitive flag
#[test]
fn test_regex_literal_ignore_case() {
    assert_runs_with_output(
        r#"
use system.text

let r_case = re"hello"i
let r_no_case = re"hello"
if r_case.matches("HELLO")
    println("case_match")
else
    println("case_no_match")
if r_no_case.matches("HELLO")
    println("no_case_match")
else
    println("no_case_no_match")
"#,
        "case_match\nno_case_no_match",
    );
}

/// Criterion 3: Dotall and multiline flags
#[test]
fn test_regex_literal_dotall_flag() {
    assert_runs_with_output(
        r#"
use system.text

let r_dotall = re"a.c"s
if r_dotall.matches("a
c")
    println("dotall_match")
else
    println("dotall_no_match")
"#,
        "dotall_match",
    );
}

#[test]
fn test_regex_literal_multiline_flag() {
    assert_runs_with_output(
        r#"
use system.text

let r_multiline = re"^b"m
if r_multiline.matches("a
b")
    println("multiline_match")
else
    println("multiline_no_match")
"#,
        "multiline_match",
    );
}

/// Criterion 4: Regex literal methods
#[test]
fn test_regex_literal_find_all() {
    assert_runs_with_output(
        r#"
use system.text

let r = re"\d+"
let matches = r.find_all("a1b22c333")
println(f"{matches.length()}")
if matches.length() == 3
    var i = 0
    while i < matches.length()
        let m = matches.element_at(i)
        println(m.text())
        i = i + 1
"#,
        "3\n1\n22\n333",
    );
}

/// Criterion 5: Malformed pattern compile error
#[test]
fn test_regex_literal_invalid_pattern() {
    assert_compiler_error(
        r#"
use system.text

let r = re"[invalid"
"#,
        "unclosed character class",
    );
}

/// Criterion 6: Global flag rejection
#[test]
fn test_regex_literal_global_flag_rejected() {
    assert_compiler_error(
        r#"
use system.text

let r = re"hi"g
"#,
        "does not support the 'g' flag",
    );
}

/// Criterion 7: Regex literal in gpu fn
#[test]
fn test_regex_literal_gpu_fn_rejected() {
    assert_compiler_error(
        r#"
use system.text

public gpu fn test_kernel()
    let r = re"pattern"
"#,
        "cannot be used inside a GPU function",
    );
}

/// Criterion 8: Caching in loop
#[test]
fn test_regex_literal_caching() {
    assert_runs_with_output(
        r#"
use system.text

var count = 0
var i = 0
while i < 10000
    let r = re"\d+"
    if r.matches("123")
        count = count + 1
    i = i + 1
if count == 10000
    println("all_matched")
else
    println("mismatch")
"#,
        "all_matched",
    );
}

/// Criterion 9: Backward compatibility with Regex.compile()
#[test]
fn test_regex_compile_still_returns_result() {
    assert_runs_with_output(
        r#"
use system.text

let result = Regex.compile("[0-9]+")
match result
    Result.Ok(r): println("ok")
    Result.Err(e): println("err")
"#,
        "ok",
    );
}

/// A regex literal needs no `use`: the class it is built through is loaded
/// with the prelude's definitions, as a collection literal's is.
#[test]
fn test_regex_literal_needs_no_import() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let r = re"[0-9]+"
    if r.matches("a1"): println("yes")
"#,
        "yes",
    );
}

/// A regex pattern in a `match` arm needs no `use` either.
#[test]
fn test_regex_pattern_needs_no_import() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    match "ab12"
        re"[0-9]+": println("digits")
        _: println("none")
"#,
        "digits",
    );
}

/// The regex class stays private without a `use`: only the literal reaches it.
#[test]
fn test_regex_type_name_still_needs_an_import() {
    assert_compiler_error(
        r#"
use system.io

fn main()
    let r = Regex.compile("[0-9]+")
"#,
        "Regex",
    );
}

/// With no class declaring a `@regex_literal` method (a standard library
/// whose prelude loads nothing), a regex literal is refused by the type
/// checker with that reason, never left for lowering to fail on.
#[test]
fn test_regex_literal_without_a_declaring_class_is_refused() {
    let stdlib = tempfile::tempdir().expect("a temporary directory");
    let system = stdlib.path().join("system");
    std::fs::create_dir(&system).expect("the system directory");
    for prelude in [
        "prelude.mi",
        "prelude_internal.mi",
        "prelude_shadowable.mi",
        "prelude_regex.mi",
    ] {
        std::fs::write(system.join(prelude), "").expect("an empty prelude file");
    }
    let result = crate::utils::miri_run_with_env(
        "fn main()\n    let _r = re\"[0-9]+\"\n",
        "MIRI_STDLIB_PATH",
        stdlib.path().to_str().expect("a UTF-8 path"),
    );
    assert!(!result.success, "the program should not compile");
    assert!(
        result
            .output()
            .contains("no class in scope declares a `@regex_literal` method"),
        "{}",
        result.output()
    );
}

/// The regex module is loaded only for a program that writes regex syntax, so
/// a program declaring its own `Result` and no regex keeps its own.
#[test]
fn test_a_program_without_regex_keeps_its_own_result() {
    assert_runs_with_output(
        r#"
use system.io

enum Result
    Yes
    No

fn main()
    match Result.No
        Result.Yes: println("yes")
        Result.No: println("no")
"#,
        "no",
    );
}

/// A regex literal written only in an imported module loads the regex module
/// for that module too.
#[test]
fn test_a_regex_literal_in_an_imported_module_needs_no_import() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                "use system.io\nuse local.digits as digits\n\nfn main()\n    println(f\"{digits.has_digit('a1')}\")\n",
            ),
            (
                "digits.mi",
                "public fn has_digit(text String) bool\n    re\"[0-9]\".matches(text)\n",
            ),
        ],
        "true",
    );
}

/// The regex module is written against the standard `Result`, so a program
/// that writes regex syntax and declares its own `Result` is told so at the
/// declaration rather than failing inside library source.
#[test]
fn test_regex_syntax_beside_an_own_result_names_the_conflict() {
    assert_compiler_error(
        r#"
use system.io

enum Result
    Yes

fn main()
    if re"[0-9]+".matches("a1"): println("yes")
"#,
        "Type 'Result' is declared in this program and also provided by",
    );
}
