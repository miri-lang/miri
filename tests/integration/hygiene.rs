// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What an edit leaves behind.
//!
//! The residue of rewriting a program is orphaned code: the helper nothing
//! calls any more, the import a rename made pointless, the binding whose last
//! reader was deleted. None of it is wrong — the program means exactly what it
//! would mean with the residue removed — so each is reported as a warning and
//! the file still compiles and runs.
//!
//! Every check here has a spelling that turns it off, because some of the
//! residue is deliberate: a name beginning with `_` says the binding is not
//! meant to be read.

use super::utils::*;

/// Assert that `code` checks clean and raises no hygiene warning of `code_str`.
///
/// The clean check is half the assertion: a program that fails to compile
/// carries no hygiene warning either, so without it the test would pass on a
/// broken fixture and prove nothing.
fn assert_no_warning(source: &str, code_str: &str) {
    let result = crate::utils::miri_check(source);
    let output = result.output();
    assert!(
        result.success,
        "the fixture must compile for its warnings to mean anything:\n{}",
        output
    );
    assert!(
        !output.contains(code_str),
        "expected no {} for this program:\n{}",
        code_str,
        output
    );
}

#[test]
fn test_a_local_nothing_reads_is_reported() {
    let code = r#"
fn main()
    let unread = 41
    println("done")
"#;
    assert_compiler_warning(code, "MER_TYP_071");
}

#[test]
fn test_the_report_names_the_unread_local() {
    let code = r#"
fn main()
    let unread = 41
    println("done")
"#;
    assert_compiler_warning(code, "unread");
}

#[test]
fn test_an_unread_local_is_a_warning_and_still_runs() {
    let code = r#"
fn main()
    let unread = 41
    println("ran")
"#;
    assert_runs_with_output(code, "ran");
}

#[test]
fn test_a_local_read_once_is_not_reported() {
    let code = r#"
fn main()
    let read = 41
    println(f"{read}")
"#;
    assert_no_warning(code, "MER_TYP_071");
}

#[test]
fn test_a_local_read_only_by_a_nested_block_is_not_reported() {
    let code = r#"
fn main()
    let read = 41
    if read > 0
        println("positive")
"#;
    assert_no_warning(code, "MER_TYP_071");
}

#[test]
fn test_a_local_named_with_a_leading_underscore_is_not_reported() {
    let code = r#"
fn main()
    let _unread = 41
    println("done")
"#;
    assert_no_warning(code, "MER_TYP_071");
}

#[test]
fn test_a_parameter_the_body_never_reads_is_reported() {
    let code = r#"
fn greet(name String) String
    "hello"

fn main()
    println(greet("world"))
"#;
    assert_compiler_warning(code, "MER_TYP_072");
}

#[test]
fn test_a_parameter_named_with_a_leading_underscore_is_not_reported() {
    let code = r#"
fn greet(_name String) String
    "hello"

fn main()
    println(greet("world"))
"#;
    assert_no_warning(code, "MER_TYP_072");
}

#[test]
fn test_a_parameter_the_body_reads_is_not_reported() {
    let code = r#"
fn greet(name String) String
    f"hello {name}"

fn main()
    println(greet("world"))
"#;
    assert_no_warning(code, "MER_TYP_072");
}

#[test]
fn test_a_declaration_without_a_body_has_no_unused_parameters() {
    let code = r#"
trait Greeter
    fn greet(name String) String

class Loud implements Greeter
    public fn greet(name String) String
        f"HELLO {name}"

fn main()
    let loud = Loud()
    println(loud.greet("world"))
"#;
    assert_no_warning(code, "MER_TYP_072");
}

#[test]
fn test_an_import_nothing_uses_is_reported() {
    let code = r#"
use system.math

fn main()
    println("done")
"#;
    assert_compiler_warning(code, "MER_IMP_005");
}

#[test]
fn test_an_import_used_once_is_not_reported() {
    let code = r#"
use system.math

fn main()
    println(f"{abs(-2)}")
"#;
    assert_no_warning(code, "MER_IMP_005");
}

#[test]
fn test_a_selected_name_nothing_uses_is_reported() {
    let code = r#"
use system.testing.{assert_eq, assert_ne}

fn main()
    assert_eq(1, 1)
"#;
    assert_compiler_warning(code, "MER_IMP_005");
}

#[test]
fn test_an_import_used_only_in_a_type_annotation_is_not_reported() {
    let code = r#"
use system.collections.list

fn main()
    let numbers List<int> = List<int>()
    println(f"{numbers.length()}")
"#;
    assert_no_warning(code, "MER_IMP_005");
}

#[test]
fn test_a_private_function_nothing_calls_is_reported() {
    let code = r#"
private fn orphan() int
    41

fn main()
    println("done")
"#;
    assert_compiler_warning(code, "MER_TYP_073");
}

#[test]
fn test_a_private_function_something_calls_is_not_reported() {
    let code = r#"
private fn helper() int
    41

fn main()
    println(f"{helper()}")
"#;
    assert_no_warning(code, "MER_TYP_073");
}

#[test]
fn test_a_public_declaration_nothing_calls_is_not_reported() {
    let code = r#"
fn exported() int
    41

fn main()
    println("done")
"#;
    assert_no_warning(code, "MER_TYP_073");
}

#[test]
fn test_a_statement_after_a_return_is_reported() {
    let code = r#"
fn announce()
    println("first")
    return
    println("never")

fn main()
    announce()
"#;
    assert_compiler_warning(code, "MER_TYP_074");
}

#[test]
fn test_a_statement_after_a_break_is_reported() {
    let code = r#"
fn main()
    var index = 0
    while index < 3
        break
        println("never")
    println(f"{index}")
"#;
    assert_compiler_warning(code, "MER_TYP_074");
}

#[test]
fn test_a_return_that_ends_its_block_is_not_reported() {
    let code = r#"
fn answer(flag bool) int
    if flag
        return 41
    return 7

fn main()
    println(f"{answer(true)}")
"#;
    assert_no_warning(code, "MER_TYP_074");
}

#[test]
fn test_a_hygiene_warning_leaves_the_check_successful() {
    let code = r#"
use system.math

fn main()
    let unread = 41
    println("ran")
"#;
    let result = crate::utils::miri_check(code);
    assert!(
        result.success,
        "warnings never make a check fail:\n{}",
        result.output()
    );
}

#[test]
fn test_an_aliased_import_nothing_uses_is_reported() {
    let code = r#"
use system.math as M

fn main()
    println("done")
"#;
    assert_compiler_warning(code, "MER_IMP_005");
}

#[test]
fn test_a_wildcard_import_is_never_reported() {
    let code = r#"
use system.math.*

fn main()
    println("done")
"#;
    assert_no_warning(code, "MER_IMP_005");
}

#[test]
fn test_a_file_of_nothing_but_imports_re_exports_them() {
    let code = r#"
use system.math
use system.io
"#;
    assert_no_warning(code, "MER_IMP_005");
}

#[test]
fn test_a_statement_after_a_continue_is_reported() {
    let code = r#"
fn main()
    var index = 0
    while index < 3
        index = index + 1
        continue
        println("never")
    println("done")
"#;
    assert_compiler_warning(code, "MER_TYP_074");
}

#[test]
fn test_a_statement_after_a_return_inside_a_branch_is_reported() {
    let code = r#"
fn pick(flag bool) int
    if flag
        return 1
        println("never")
    return 2

fn main()
    println(f"{pick(true)}")
"#;
    assert_compiler_warning(code, "MER_TYP_074");
}

/// Every `main` gets a `return 0` appended so the process exits cleanly. It
/// carries no span, because nobody wrote it — and it follows the author's own
/// `return` in every program that ends with one.
#[test]
fn test_the_return_the_compiler_appends_to_main_is_not_reported() {
    let code = r#"
fn main()
    println("done")
    return
"#;
    assert_no_warning(code, "MER_TYP_074");
}

#[test]
fn test_a_private_class_nothing_names_is_reported() {
    let code = r#"
private class Hidden
    var x int

fn main()
    println("done")
"#;
    assert_compiler_warning(code, "MER_TYP_073");
}

#[test]
fn test_a_private_method_reached_through_self_is_not_reported() {
    let code = r#"
class Counter
    var n int
    private fn bump() int
        return self.n + 1
    public fn show() int
        return self.bump()

fn main()
    let counter = Counter(n: 1)
    println(f"{counter.show()}")
"#;
    assert_no_warning(code, "MER_TYP_073");
}

/// A binding at the file's own top level is part of what the module offers,
/// reachable by everything that imports it, so it is not a local that went
/// unread.
#[test]
fn test_a_module_scope_binding_is_not_reported_as_an_unread_local() {
    let code = r#"
const LIMIT = 10

public fn describe() int
    return 1
"#;
    assert_no_warning(code, "MER_TYP_071");
}

#[test]
fn test_a_private_module_scope_binding_nothing_reads_is_reported() {
    let code = r#"
private const LIMIT = 10

public fn describe() int
    return 1
"#;
    assert_compiler_warning(code, "MER_TYP_073");
}

#[test]
fn test_a_name_bound_by_a_match_arm_is_not_reported() {
    let code = r#"
fn get() Result<int, String>
    return Result.Ok(1)

fn main()
    let outcome = get()
    match outcome
        Result.Ok(value): println(f"{value}")
        Result.Err(problem): println(problem)
"#;
    assert_no_warning(code, "MER_TYP_071");
}

#[test]
fn test_a_parameter_read_only_by_another_parameters_default_is_not_reported() {
    let code = r#"
fn add(a int, b int = a) int
    return b

fn main()
    println(f"{add(2)}")
"#;
    assert_no_warning(code, "MER_TYP_072");
}

#[test]
fn test_an_import_used_only_by_an_implements_clause_is_not_reported() {
    let code = r#"
use system.accelerator

struct Point implements Accelerable
    x int

fn main()
    let point = Point(x: 1)
    println(f"{point.x}")
"#;
    assert_no_warning(code, "MER_IMP_005");
}

#[test]
fn test_an_out_parameter_the_body_writes_is_not_reported() {
    let code = r#"
fn split(total int, half out int)
    half = total / 2

fn main()
    var result = 0
    split(10, result)
    println(f"{result}")
"#;
    assert_no_warning(code, "MER_TYP_072");
}
