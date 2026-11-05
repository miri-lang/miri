// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A closure that hands one of its captures to an `out` parameter writes that
//! capture: the callee replaces the value, so the closure owns the new one and
//! releases it once, and the value the capture held before is released once.

use super::super::utils::*;

#[test]
fn a_captured_list_refilled_through_an_out_parameter_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn refill(xs out [int])
    xs = List([4, 5, 6, 7])

fn main()
    var xs = List([1, 2, 3])
    let f = fn() int
        refill(xs)
        xs.length()
    println(f"{f()}")
"#,
        "4",
    );
}

#[test]
fn a_closure_refilling_its_capture_on_every_call_releases_each_value_once() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn refill(xs out [int])
    xs = List([9])

fn main()
    var xs = List([1, 2, 3])
    let f = fn() int
        refill(xs)
        xs[0]
    println(f"{f()} {f()}")
"#,
        "9 9",
    );
}

#[test]
fn a_captured_string_rewritten_through_an_out_parameter_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.io

fn shout(s out String)
    s = s + "!"

fn main()
    var s = "hi" + ""
    let f = fn() String
        shout(s)
        s
    println(f())
"#,
        "hi!",
    );
}
