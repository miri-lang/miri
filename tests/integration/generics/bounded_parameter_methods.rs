// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method called on a trait-bounded type parameter runs the method of the
//! class the parameter is bound to, compiled at that class instance's own
//! arguments. It is checked there, at every call that binds the parameter —
//! directly or through another generic function that hands its own bounded
//! parameter on. Without the check, a `Box<String>` would run a `lt` that
//! compares a string's address with an integer.

use super::utils::*;

const CANNOT_COMPARE: &str = "cannot compare String and int";

const BOUNDED: &str = r#"
use system.io

trait Lt
    fn lt() bool

class Box<T> implements Lt
    v T
    fn lt() bool
        return self.v < 10

fn go<X implements Lt>(x X) bool
    return x.lt()
"#;

fn with(main: &str) -> String {
    format!("{BOUNDED}\n{main}")
}

#[test]
fn a_bounded_parameters_method_is_refused_at_the_call_that_binds_it() {
    assert_compiler_error(
        &with(
            r#"
fn main()
    let b = Box<String>(v: "x" + "y")
    println(f"{go(b)}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_bounded_parameters_method_is_refused_by_a_build_too() {
    assert_build_error(
        &with(
            r#"
fn main()
    let b = Box<String>(v: "x" + "y")
    println(f"{go(b)}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_bounded_parameters_method_runs_at_a_valid_argument() {
    assert_heap_guard_output(
        &with(
            r#"
fn main()
    println(f"{go(Box<int>(v: 3))}")
"#,
        ),
        "true",
    );
}

#[test]
fn a_bounded_parameter_handed_on_is_refused_where_the_outer_call_binds_it() {
    assert_compiler_error(
        &with(
            r#"
fn outer<Y implements Lt>(y Y) bool
    return go(y)

fn main()
    let b = Box<String>(v: "x" + "y")
    println(f"{outer(b)}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_bounded_parameters_method_that_never_touches_the_argument_is_accepted() {
    assert_heap_guard_output(
        r#"
use system.io

trait Named
    fn name() String

class Box<T> implements Named
    v T
    fn name() String
        return "box"

fn describe<X implements Named>(x X) String
    return x.name()

fn main()
    let b = Box<String>(v: "x" + "y")
    println(describe(b))
"#,
        "box",
    );
}
