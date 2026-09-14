// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Storing a bare value into an array of optionals.
//!
//! The value has to arrive as `Some(value)`; kept raw, the next read treats the
//! payload as a pointer to an optional and faults.

use super::utils::*;

#[test]
fn array_set_and_index_assign_wrap_the_stored_value() {
    assert_runs_with_output(
        r#"
fn main()
    var xs = [Some(1), Some(2)]
    xs.set(0, 4)
    xs[1] = 5
    println(f"{xs[0]} {xs[1]}")
"#,
        "Some(4) Some(5)",
    );
}
