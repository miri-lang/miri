// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_tuple_requires_import_for_methods() {
    assert_compiler_error(
        r#"
let t = (1, 2, 3)
println(f"{t.length()}")
"#,
        "does not have members",
    );
}

#[test]
fn test_tuple_element_at_oob() {
    // OOB element_at triggers a runtime "Array index out of bounds" error.
    assert_runtime_crash(
        r#"
use system.collections.tuple
let t = (1, 2, 3)
let x = t.element_at(5)
"#,
    );
}

#[test]
fn test_tuple_access_oob_compiler_error() {
    assert_compiler_error(
        r#"
let t = (1, 2, 3)
let x = t.3
"#,
        "Tuple index out of bounds",
    );
}

#[test]
fn test_heterogeneous_tuple_methods_error() {
    // Methods in system.collections.tuple are defined for Tuple<T>.
    // A heterogeneous tuple (int, String) correctly does not support these methods.
    assert_compiler_error(
        r#"
use system.collections.tuple
let t = (1, "hello")
println(f"{t.contains(1)}")
"#,
        "does not have members",
    );
}

#[test]
fn test_tuple_mutation_restriction() {
    assert_compiler_error(
        r#"
let t = (1, 2)
t.0 = 10
"#,
        "Cannot assign to field of immutable variable",
    );
}

#[test]
fn test_a_tuple_element_cannot_be_assigned_by_index() {
    assert_compiler_error(
        r#"
var t = (1, 2)
t[0] = 5
"#,
        "a tuple's elements are fixed when it is built",
    );
}

#[test]
fn test_a_tuple_element_cannot_be_assigned_by_position() {
    assert_compiler_error(
        r#"
var t = (1, "a")
t.1 = "b"
"#,
        "a tuple's elements are fixed when it is built",
    );
}

#[test]
fn test_a_new_tuple_replaces_one_with_a_changed_element() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    var t = (1, "a" + "b")
    let u = t
    t = (5, t.1)
    println(f"{t.0} {t.1} {u.0}")
"#,
        "5 ab 1",
    );
}
