// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A closure may hand a local holding a copy of one of its captures to an
//! `out` parameter: the callee replaces the value, so the closure owns the
//! new one and releases it once, and the value the local held before is
//! released once. Handing it the capture itself is refused (`MER_TYP_090`).

use super::super::utils::*;

#[test]
fn a_copy_of_a_captured_list_refilled_through_an_out_parameter_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn refill(xs out [int])
    xs = List([4, 5, 6, 7])

fn main()
    var xs = List([1, 2, 3])
    let f = fn() int
        var mine = xs
        refill(mine)
        mine.length()
    println(f"{f()}")
"#,
        "4",
    );
}

#[test]
fn a_closure_refilling_a_copy_of_its_capture_on_every_call_releases_each_value_once() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn refill(xs out [int])
    xs = List([9])

fn main()
    var xs = List([1, 2, 3])
    let f = fn() int
        var mine = xs
        refill(mine)
        mine[0]
    println(f"{f()} {f()}")
"#,
        "9 9",
    );
}

#[test]
fn a_copy_of_a_captured_string_rewritten_through_an_out_parameter_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.io

fn shout(s out String)
    s = s + "!"

fn main()
    var s = "hi" + ""
    let f = fn() String
        var mine = s
        shout(mine)
        mine
    println(f())
"#,
        "hi!",
    );
}
