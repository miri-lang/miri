// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A number literal binds a type parameter only when no other argument does,
//! so it takes the width of the argument beside it, as it does beside an
//! operator: the order the arguments are written in does not decide the type.

use super::utils::*;

#[test]
fn a_float_literal_before_an_f32_argument_takes_its_width() {
    assert_runs_with_output(
        r#"
use system.io

fn first<T>(a T, b T) T
    return a

fn main()
    let b f32 = 1.5
    let x f32 = first(0.5, b)
    println(f"{x}")
"#,
        "0.5",
    );
}

#[test]
fn an_integer_literal_before_an_i64_argument_takes_its_width() {
    assert_runs_with_output(
        r#"
use system.io

fn first<T>(a T, b T) T
    return a

fn main()
    let b i64 = 9
    let x i64 = first(7, b)
    println(f"{x}")
"#,
        "7",
    );
}

/// With only literals, the first one still decides, as before.
#[test]
fn literals_alone_bind_the_parameter_at_their_own_type() {
    assert_runs_with_output(
        r#"
use system.io

fn first<T>(a T, b T) T
    return a

fn main()
    let x float = first(0.5, 1.5)
    println(f"{x}")
"#,
        "0.5",
    );
}

/// A literal that does not fit the type its sibling binds is refused, never
/// converted.
#[test]
fn a_float_literal_beside_an_int_argument_is_refused() {
    assert_compiler_error(
        r#"
fn first<T>(a T, b T) T
    return a

fn main()
    let n = 3
    let x = first(1.5, n)
"#,
        "expected int",
    );
}
