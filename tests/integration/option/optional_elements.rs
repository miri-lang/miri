// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A literal whose elements are plain values, built where the element type is
//! optional, boxes each element as `Some` on the way: the collection holds
//! optionals, which is what every reader of it expects.

use super::super::utils::*;

#[test]
fn test_an_array_literal_of_plain_ints_built_as_optionals_holds_somes() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let xs Array<int?, 2> = [1, 2]
    match xs[1]
        Some(v): println(f"second {v}")
        None: println("none")
    println(f"{xs[0] ?? 0}")
"#,
        "second 2\n1",
    );
}

#[test]
fn test_a_map_literal_of_plain_values_built_as_optionals_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

fn main()
    var m {String: int?} = {"a": 1}
    m["b"] = None
    let a = m["a"] ?? -1
    println(f"{m.length()} {a}")
"#,
        "2 1",
    );
}
