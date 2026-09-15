// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_returning_an_optional_as_an_optional_of_optional_wraps_it() {
    assert_heap_guard_output(
        r#"
fn lift(o int?) Option<int?>
    return o

fn main()
    match lift(Some(9))
        Some(inner): println(f"inner {inner}")
        None: println("outer none")
"#,
        "inner Some(9)",
    );
}

#[test]
fn test_lifting_none_is_some_none_and_a_none_literal_is_the_outer_none() {
    assert_heap_guard_output(
        r#"
fn lift(o int?) Option<int?>
    return o

fn nothing() Option<int?>
    return None

fn show(o Option<int?>)
    match o
        Some(inner): match inner
            Some(v): println(f"some some {v}")
            None: println("some none")
        None: println("none")

fn main()
    show(lift(None))
    show(nothing())
    show(lift(Some(4)))
"#,
        "some none\nnone\nsome some 4",
    );
}

#[test]
fn test_an_optional_initializer_for_an_optional_of_optional_is_wrapped() {
    assert_heap_guard_output(
        r#"
fn main()
    let inner int? = None
    let o Option<int?> = inner
    match o
        Some(i): println(f"some {i}")
        None: println("none")
"#,
        "some None",
    );
}

#[test]
fn test_first_and_last_over_a_list_of_optional_ints() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn show(o Option<int?>)
    match o
        Some(inner): match inner
            Some(v): println(f"some some {v}")
            None: println("some none")
        None: println("none")

fn main()
    var xs = List<int?>()
    show(xs.first())
    xs.push(Some(9))
    xs.push(None)
    show(xs.first())
    show(xs.last())
    let f = xs.first()
    println(f"{f}")
"#,
        "none\nsome some 9\nsome none\nSome(Some(9))",
    );
}

#[test]
fn test_first_and_last_over_a_list_of_optional_strings() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn show(o Option<String?>)
    match o
        Some(inner): match inner
            Some(v): println(f"some some {v}")
            None: println("some none")
        None: println("none")

fn main()
    var xs = List<String?>()
    show(xs.last())
    xs.push(None)
    xs.push(Some("tail"))
    show(xs.first())
    show(xs.last())
"#,
        "none\nsome none\nsome some tail",
    );
}

#[test]
fn test_a_bare_value_returned_or_passed_where_option_is_spelled_is_boxed() {
    assert_heap_guard_output(
        r#"
fn five() Option<int>
    return 5

fn take(o Option<int>)
    match o
        Some(v): println(f"take {v}")
        None: println("take none")

fn main()
    match five()
        Some(v): println(f"five {v}")
        None: println("none")
    take(7)
    take(None)
"#,
        "five 5\ntake 7\ntake none",
    );
}

#[test]
fn test_a_lifted_optional_passed_straight_to_a_call_is_released_once() {
    assert_heap_guard_output(
        r#"
fn lift(o int?) Option<int?>
    return o

fn make() Option<String?>
    return Some(Some("made"))

fn peek(o Option<int?>)
    match o
        Some(inner): println(f"peek {inner}")
        None: println("peek none")

fn show(o Option<String?>)
    match o
        Some(inner): println(f"show {inner}")
        None: println("show none")

fn main()
    peek(lift(Some(1)))
    show(make())
"#,
        "peek Some(1)\nshow Some(made)",
    );
}

#[test]
fn test_nested_optionals_of_strings_are_heap_guard_clean() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn lift(o String?) Option<String?>
    return o

fn show(o Option<String?>)
    match o
        Some(inner): match inner
            Some(v): println(f"some some {v}")
            None: println("some none")
        None: println("none")

fn main()
    var xs = List<String?>()
    xs.push(Some("head"))
    xs.push(None)
    show(xs.first())
    show(xs.last())
    show(lift(Some("lifted")))
    show(lift(None))
"#,
        "some some head\nsome none\nsome some lifted\nsome none",
    );
}
