// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

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

#[test]
fn test_set_constructor_rejects_non_literal_arg() {
    // `Set(non-literal)` is not supported because lowering delegates to the set-literal
    // lowering. Passing an arbitrary value of `Set<T>` would silently produce an empty set.
    assert_compiler_error(
        "
use system.collections.set

let other = {1, 2, 3}
let s = Set(other)
",
        "only accepts a set literal",
    );
}
