// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_set_passed_to_and_returned_from_function() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

fn modify_and_return_set(s Set<int>) Set<int>
    s.add(99)
    return s

fn main()
    let s = {1, 2, 3}
    let s2 = modify_and_return_set(s)
    println(f"{s2.length()}")
    println(f"{s2.contains(99)}")
    println(f"{s.contains(99)}") // because it's passed by reference
"#,
        "4\ntrue\ntrue",
    );
}
