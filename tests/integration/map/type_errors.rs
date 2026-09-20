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

#[test]
fn map_with_type_arguments_rejects_scalar_arg() {
    // Same shape as the set constructor: the type arguments were taken as the
    // whole answer, so the positional argument went unchecked and unused.
    assert_compiler_error(
        "
use system.collections.map

fn main()
    let m = Map<int, int>(7)
    println(f\"{m.length()}\")
",
        "expects a map literal of 'int' to 'int'",
    );
}

#[test]
fn map_with_type_arguments_rejects_mismatched_value_type() {
    assert_compiler_error(
        "
use system.collections.map

fn main()
    let m = Map<String, int>({\"a\": \"b\"})
    println(f\"{m.length()}\")
",
        "expects a map literal of 'String' to 'int'",
    );
}

#[test]
fn map_with_type_arguments_accepts_a_matching_literal() {
    assert_runs_with_output(
        "
use system.collections.map

fn main()
    let m = Map<String, int>({\"a\": 1})
    let got = m[\"a\"]
    println(f\"{m.length()} {got}\")
",
        "1 1",
    );
}
