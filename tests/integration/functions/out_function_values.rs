// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A function value is called through the type it is held at, and a function
//! type cannot declare `out`, so a function taking a parameter `out` is
//! refused where a function type is expected: the call through the value
//! would hand it a value where it reads the address of a slot.

use super::utils::*;
use crate::utils::miri_check_json;

const OUT_FOLD: &str = r#"
use system.io
use system.collections.list

fn fold(acc out int, x int) int
    acc = acc + x
    return acc

fn apply(f fn(int, int) int) int
    return f(1, 2)
"#;

fn with_out_fold(program: &str) -> String {
    format!("{OUT_FOLD}\n{program}")
}

const OUT_FOLD_HELP: &str = "a function value takes each parameter as its type `Function(int, int) -> int` declares, and a function type cannot declare `out`: pass a function that takes the value as a plain parameter and returns the new one";

#[test]
fn a_function_taking_out_passed_as_a_value_type_without_it_is_refused() {
    let source = with_out_fold(
        r#"
fn main()
    println(f"{apply(fold)}")
"#,
    );
    let envelope = miri_check_json(&source);
    let diagnostic = (envelope.diagnostics.iter())
        .find(|d| d.code.as_deref() == Some("MER_TYP_002"))
        .unwrap_or_else(|| panic!("a type mismatch in:\n{source}"));
    assert_eq!(
        diagnostic.message,
        "Type mismatch for argument 'f': expected Function(int, int) -> int, got Function(out int, int) -> int"
    );
    assert_eq!(diagnostic.help.as_deref(), Some(OUT_FOLD_HELP));
}

#[test]
fn a_function_taking_out_handed_to_a_runtime_closure_is_refused() {
    assert_compiler_error(
        &with_out_fold(
            r#"
fn main()
    let r = List([1, 2, 3, 4]).reduce(0, fold)
    println(f"{r}")
"#,
        ),
        "got Function(out int, int) -> int",
    );
}

#[test]
fn a_function_taking_the_value_and_returning_the_new_one_runs_as_a_value() {
    assert_runs_with_output(
        &with_out_fold(
            r#"
fn added(acc int, x int) int
    return acc + x

fn main()
    println(f"{apply(added)} {List([1, 2, 3, 4]).reduce(0, added)}")
"#,
        ),
        "3 10",
    );
}
