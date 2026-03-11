// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_set_alias_no_double_free() {
    assert_runs(
        r#"
use system.collections.set
let s1 = {1, 2, 3}
let s2 = s1 // RC increments
// Both go out of scope, shouldn't double free
"#,
    );
}

#[test]
fn test_set_reassign_frees_old() {
    assert_runs(
        r#"
use system.collections.set
var s = {1, 2, 3}
s = {4, 5} // old set should be freed here
"#,
    );
}

#[test]
fn test_set_passed_to_function_no_dangle() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

fn consume(s Set<int>)
    // does nothing, reference goes out of scope, RC decremented

fn main()
    let s = {10, 20, 30}
    consume(s)
    // s should still be valid here
    println(f"{s.length()}")
"#,
        "3",
    );
}

#[test]
fn test_set_aliasing_mutation() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
var s1 = {1}
let s2 = s1

// Mutate through s1, s2 should see the change
s1.add(2)
println(f"{s2.contains(2)}")
println(f"{s2.length()}")
"#,
        "true\n2",
    );
}
