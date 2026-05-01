// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_wrong_key_type() {
    assert_compiler_error(
        r#"
let m = {"a": 1, "b": 2}
let x = m[42]
"#,
        "Invalid map key type",
    );
}

#[test]
fn map_lowercase_type_not_allowed() {
    assert_compiler_error(
        r#"
fn get(m map<String, int>) int
    return 0
"#,
        "",
    );
}

#[test]
fn map_constructor_rejects_non_literal_arg() {
    // `Map(non-literal)` is not supported because lowering delegates to the map-literal
    // lowering. Passing an arbitrary value of `Map<K, V>` would silently produce an empty map.
    assert_compiler_error(
        r#"
use system.collections.map

let other = {"a": 1}
let m = Map(other)
"#,
        "only accepts a map literal",
    );
}
