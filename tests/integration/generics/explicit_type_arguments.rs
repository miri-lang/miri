// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Calling a generic function with its type arguments written out — `f<int>(x)`
//! — means the same call as `f(x)` with the argument inferred. The spelling
//! parses as a generic type reference wrapping the function's name, so the name
//! has to be recovered before the call is lowered; left wrapped, it reaches the
//! arm that refuses a type used as a value.
//!
//! Writing the arguments out is the only way to reach an instantiation nothing
//! else pins — a function whose parameter list mentions the parameter in no
//! position an argument can be inferred from.

use super::utils::*;

#[test]
fn an_explicit_type_argument_calls_the_same_body_inference_would() {
    assert_runs_with_output(
        r#"
fn ident<T>(a T) T
    return a

fn main()
    let inferred = ident(5)
    let written = ident<int>(5)
    println(f"{inferred} {written}")
"#,
        "5 5",
    );
}

#[test]
fn an_explicit_type_argument_reaches_an_instantiation_nothing_else_pins() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn empty<T>() [T]
    return List<T>()

fn main()
    let xs = empty<int>()
    println(f"{xs.length()}")
"#,
        "0",
    );
}

#[test]
fn an_explicit_type_argument_passes_a_sized_array_parameter() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn fill<T>(a Array<T, 4>) int
    return a.length()

fn main()
    let a = Array<int, 4>()
    println(f"{fill<int>(a)}")
"#,
        "4",
    );
}

#[test]
fn an_explicit_type_argument_returns_a_sized_array() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn make<T>() Array<T, 4>
    var a = Array<T, 4>()
    return a

fn main()
    println(f"{make<int>().length()}")
"#,
        "4",
    );
}

#[test]
fn a_sized_array_round_trips_through_a_generic_function() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn through<T>(a Array<T, 4>) Array<T, 4>
    return a

fn main()
    var a = Array<int, 4>()
    a[2] = 7
    let b = through<int>(a)
    println(f"{b[2]} {b.length()}")
"#,
        "7 4",
    );
}

/// A sized array holds its elements zeroed at construction, so its element
/// type has to be one a zero is a value of — a managed element is refused at
/// the declaration, not here. Two scalar widths are what distinguishes two
/// bodies: a body compiled for the wrong one reads the other's bits.
#[test]
fn two_element_types_produce_two_correct_bodies() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn through<T>(a Array<T, 4>) Array<T, 4>
    return a

fn main()
    var ints = Array<int, 4>()
    ints[0] = 9
    var reals = Array<f64, 4>()
    reals[0] = 1.5
    let a = through<int>(ints)
    let b = through<f64>(reals)
    println(f"{a[0]} {b[0]}")
"#,
        "9 1.5",
    );
}

/// The managed counterpart, through the unsized spelling a managed element is
/// allowed in.
#[test]
fn an_explicit_type_argument_round_trips_a_managed_element() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn through<T>(a [T]) [T]
    return a

fn main()
    var texts = List<String>()
    texts.push("h" + "i")
    let back = through<String>(texts)
    println(f"{back.length()} {back[0]}")
"#,
        "1 hi",
    );
}

#[test]
fn an_explicit_type_argument_on_a_managed_element_releases_it() {
    assert_heap_guard_output(
        r#"
fn ident<T>(a T) T
    return a

fn main()
    let s = ident<String>("h" + "i")
    println(s)
"#,
        "hi",
    );
}
