// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Set operations whose element type is a scalar narrower or wider than `int`,
//! which compiles a per-element-width copy of the set's own methods.

use crate::integration::utils::*;

#[test]
fn test_set_i32_remove_returns_and_shrinks() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<i32>({})
    s.add(200)
    s.add(201)
    let removed = s.remove(200)
    println(f"{removed}")
    println(f"{s.length()}")
    println(f"{s.contains(201)}")
"#,
        "true
1
true",
    );
}

#[test]
fn test_set_f64_remove_returns_and_shrinks() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<f64>({})
    s.add(1.5)
    s.add(2.5)
    let removed = s.remove(1.5)
    println(f"{removed}")
    println(f"{s.length()}")
    println(f"{s.contains(2.5)}")
"#,
        "true
1
true",
    );
}

#[test]
fn test_set_int_remove_still_works() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set({1, 2})
    let removed = s.remove(1)
    println(f"{removed}")
    println(f"{s.length()}")
"#,
        "true
1",
    );
}
