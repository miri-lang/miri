// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A written local type inside a generic body means the body's own parameter.
//! `var x Tagged<T> = Tagged<int>(1)` inside `fn f<T>` declares one type and
//! initializes it with another, and the two cannot both be right — the body is
//! compiled for whatever `T` is, and nothing makes that an `int`.
//!
//! The same spelling outside a generic body is refused, so the two are checked
//! here as a pair. Splitting them is how they came apart: an unconstrained
//! parameter is treated as matching anything, which is right where the
//! parameter is the whole type and wrong inside a written type argument.

use super::utils::*;

const TAGGED: &str = r#"
class Tagged<T>
    value T

    fn init(value T)
        self.value = value
"#;

fn with_tagged(main: &str) -> String {
    format!("{TAGGED}\n{main}")
}

#[test]
fn a_written_type_argument_is_checked_against_the_initializer_inside_a_generic_body() {
    assert_compiler_error(
        &with_tagged(
            r#"
fn held<T>(a T) T
    var x Tagged<T> = Tagged<int>(1)
    return x.value

fn main()
    println(held("PEAR".to_lower()))
"#,
        ),
        "expected Tagged<T>, got Tagged<int>",
    );
}

/// The same spelling outside a generic body, which has always been refused.
/// Its message is the one the generic case now joins.
#[test]
fn a_written_type_argument_is_checked_against_the_initializer_outside_a_generic_body() {
    assert_compiler_error(
        &with_tagged(
            r#"
fn main()
    var x Tagged<String> = Tagged<int>(1)
    println(f"{x.value}")
"#,
        ),
        "expected Tagged<String>, got Tagged<int>",
    );
}

#[test]
fn a_correct_written_type_argument_still_checks_at_a_managed_instantiation() {
    assert_heap_guard_output(
        &with_tagged(
            r#"
fn held<T>(a T) T
    var x Tagged<T> = Tagged<T>(a)
    return x.value

fn main()
    println(held("h" + "i"))
"#,
        ),
        "hi",
    );
}

#[test]
fn a_correct_written_type_argument_still_checks_at_a_scalar_instantiation() {
    assert_runs_with_output(
        &with_tagged(
            r#"
fn held<T>(a T) T
    var x Tagged<T> = Tagged<T>(a)
    return x.value

fn main()
    println(f"{held(7)}")
"#,
        ),
        "7",
    );
}

/// A bare parameter as the whole declared type keeps accepting whatever the
/// body is instantiated at — that is the rule the argument position must not
/// borrow.
#[test]
fn a_bare_parameter_as_the_whole_declared_type_still_accepts_the_instantiation() {
    assert_runs_with_output(
        r#"
fn held<T>(a T) T
    var x T = a
    return x

fn main()
    println(f"{held(7)}")
    println(held("h" + "i"))
"#,
        "7\nhi",
    );
}

#[test]
fn a_written_type_argument_naming_a_different_parameter_is_refused() {
    assert_compiler_error(
        &with_tagged(
            r#"
fn pair<A, B>(a A, b B) A
    var x Tagged<A> = Tagged<B>(b)
    return x.value

fn main()
    println(f"{pair(1, 2.5)}")
"#,
        ),
        "expected Tagged<A>, got Tagged<B>",
    );
}
