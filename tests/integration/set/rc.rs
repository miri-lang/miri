// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ── Drop-fn setter wiring for Set<String> (task 2.4) ─────────────────────────

#[test]
fn test_set_of_strings_remove_no_crash() {
    // Set<String>: elem_drop_fn must be set so that remove() properly DecRefs
    // the string element instead of leaking it.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

fn main()
    var s = {"hello", "world", "foo"}
    s.remove("world")
    println(f"{s.length()}")
"#,
        "2",
    );
}

#[test]
fn test_set_of_strings_clear_no_crash() {
    // Set<String>: clear() must DecRef all string elements via elem_drop_fn.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

fn main()
    var s = {"a", "b", "c"}
    s.clear()
    println(f"{s.length()}")
"#,
        "0",
    );
}

// ── Set<int> baseline cleanup (task 2.5) ─────────────────────────────────────

#[test]
fn test_set_int_clear_no_crash() {
    // Set<int>: no managed elements — clear() is the baseline path with no
    // elem_drop_fn.  Verifies the set itself is freed without crashing.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

fn main()
    var s = {10, 20, 30, 40}
    s.clear()
    println(f"{s.length()}")
"#,
        "0",
    );
}

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
