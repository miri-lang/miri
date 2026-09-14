// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Storing a bare value into a map of optionals.
//!
//! The value has to arrive as `Some(value)`, through `set` and through an
//! index write alike; kept raw, the read treats the payload as a pointer to an
//! optional and faults.

use super::utils::*;

#[test]
fn set_and_index_write_store_some() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String, int?>()
    m.set("a", 1)
    m["b"] = 2
    let a = m["a"]
    let b = m["b"]
    println(f"{a} {b}")
    var words = Map<int, String?>()
    words.set(1, "one")
    words[2] = "two"
    let one = words[1]
    let two = words[2]
    println(f"{one} {two}")
"#,
        "Some(1) Some(2)\nSome(one) Some(two)",
    );
}
