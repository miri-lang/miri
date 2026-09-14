// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Adding a bare value to a set of optionals.
//!
//! The value has to arrive as `Some(value)`; kept raw, iterating the set treats
//! the payload as a pointer to an optional and faults.

use super::utils::*;

#[test]
fn added_values_read_back_as_some() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var xs = Set<int?>()
    xs.add(1)
    println(f"{xs.length()}")
    for x in xs
        println(f"{x}")
    var ys = Set<String?>()
    ys.add("word")
    for y in ys
        println(f"{y}")
"#,
        "1\nSome(1)\nSome(word)",
    );
}
