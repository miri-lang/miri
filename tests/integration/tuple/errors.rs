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
use system.io
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
