// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Stack<T> at non-int element widths.
//!
//! The backing list stores an element at the width its type argument declares,
//! so a float goes in and comes back out as the same bits. Reading it back at
//! the pointer-width fallback instead is what these guard against.

use crate::integration::utils::*;

#[test]
fn test_stack_float_push_pop() {
    assert_runs_with_output(
        r#"
use system.collections.stack

fn main()
    var s = Stack<float>()
    s.push(1.5)
    s.push(2.5)
    s.push(3.5)
    println(f"{s.pop() ?? 0.0}")
    println(f"{s.pop() ?? 0.0}")
    println(f"{s.pop() ?? 0.0}")
"#,
        "3.5
2.5
1.5",
    );
}
