// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Map operations whose key or value type is a scalar narrower or wider than
//! `int`.
//!
//! Such an instantiation compiles a per-element-width copy of the map's own
//! methods. A method that forwards a parameter straight to a runtime call —
//! `remove`, `set` — passes it through a temp typed from the runtime
//! declaration, which spells the map's type parameters. Left unsubstituted that
//! spelling is an unknown named type, so reference counting treats a plain
//! integer as a pointer and releases it.

use crate::integration::utils::*;

#[test]
fn test_map_i32_remove_returns_and_shrinks() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<i32, i32>({})
    m.set(1, 10)
    m.set(2, 20)
    let removed = m.remove(1)
    println(f"{removed}")
    println(f"{m.length()}")
    println(f"{m[2]}")
"#,
        "true
1
20",
    );
}

#[test]
fn test_map_i32_remove_absent_key_is_false() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<i32, i32>({})
    m.set(1, 10)
    let removed = m.remove(9)
    println(f"{removed}")
    println(f"{m.length()}")
"#,
        "false
1",
    );
}

#[test]
fn test_map_string_key_float_value_remove() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String, f64>({})
    m.set("a", 2.5)
    m.set("b", 3.5)
    let removed = m.remove("a")
    let kept = m["b"]
    println(f"{removed}")
    println(f"{kept}")
"#,
        "true
3.5",
    );
}

#[test]
fn test_map_int_remove_still_works() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map({1: 10, 2: 20})
    let removed = m.remove(1)
    println(f"{removed}")
    println(f"{m.length()}")
"#,
        "true
1",
    );
}
