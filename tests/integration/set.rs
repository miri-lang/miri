// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_compiler_error, assert_runs, assert_runs_with_output};

// =========================================================================
// Construction
// =========================================================================

#[test]
fn test_set_creation() {
    assert_runs("let s = {1, 2, 3}");
}

#[test]
fn test_set_creation_strings() {
    assert_runs("let s = {'a', 'b', 'c'}");
}

#[test]
fn test_set_creation_single() {
    assert_runs("let s = {42}");
}

// =========================================================================
// Length
// =========================================================================

#[test]
fn test_set_length() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
let s = {1, 2, 3}
println(f"{s.length()}")
"#,
        "3",
    );
}

#[test]
fn test_set_length_dedup() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
let s = {1, 2, 2, 3, 3, 3}
println(f"{s.length()}")
"#,
        "3",
    );
}

// =========================================================================
// Contains / membership
// =========================================================================

#[test]
fn test_set_contains() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
let s = {10, 20, 30}
println(f"{s.contains(20)}")
println(f"{s.contains(99)}")
"#,
        "true\nfalse",
    );
}

#[test]
fn test_set_in_operator() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
let s = {1, 2, 3}
if 2 in s
    println("yes")
"#,
        "yes",
    );
}

// =========================================================================
// Add / Remove
// =========================================================================

#[test]
fn test_set_add() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
var s = {1, 2}
s.add(3)
println(f"{s.length()}")
println(f"{s.contains(3)}")
"#,
        "3\ntrue",
    );
}

#[test]
fn test_set_add_duplicate() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
var s = {1, 2}
s.add(2)
println(f"{s.length()}")
"#,
        "2",
    );
}

#[test]
fn test_set_remove() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
var s = {1, 2, 3}
s.remove(2)
println(f"{s.length()}")
println(f"{s.contains(2)}")
"#,
        "2\nfalse",
    );
}

// =========================================================================
// is_empty / clear
// =========================================================================

#[test]
fn test_set_is_empty() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
let s = {1}
println(f"{s.is_empty()}")
"#,
        "false",
    );
}

#[test]
fn test_set_clear() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
var s = {1, 2, 3}
s.clear()
println(f"{s.length()}")
println(f"{s.is_empty()}")
"#,
        "0\ntrue",
    );
}

// =========================================================================
// Import requirements
// =========================================================================

#[test]
fn test_set_lowercase_not_recognized() {
    // Lowercase `set` is a type annotation (like `{int}`), not a class constructor.
    // Using it as a constructor should fail.
    assert_compiler_error(
        "
let s = set()
",
        "Undefined",
    );
}

#[test]
fn test_set_requires_import_for_methods() {
    assert_compiler_error(
        r#"
let s = {1, 2, 3}
println(f"{s.length()}")
"#,
        "does not have members",
    );
}
