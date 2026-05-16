// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_tuple_length() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.length()}")
"#,
        "3",
    );
}

#[test]
fn test_tuple_length_single() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (42,)
println(f"{t.length()}")
"#,
        "1",
    );
}

#[test]
fn test_tuple_length_pair() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (1, 2)
println(f"{t.length()}")
"#,
        "2",
    );
}

#[test]
fn test_tuple_element_at() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.element_at(0)}")
println(f"{t.element_at(1)}")
println(f"{t.element_at(2)}")
"#,
        "10\n20\n30",
    );
}

#[test]
fn test_tuple_is_empty() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (1, 2, 3)
println(f"{t.is_empty()}")
"#,
        "false",
    );
}

#[test]
fn test_tuple_first() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.first() ?? -1}")
"#,
        "10",
    );
}

#[test]
fn test_tuple_last() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.last() ?? -1}")
"#,
        "30",
    );
}

#[test]
fn test_tuple_first_single() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (42,)
println(f"{t.first() ?? -1}")
println(f"{t.last() ?? -1}")
"#,
        "42\n42",
    );
}

#[test]
fn test_tuple_contains() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.contains(20)}")
println(f"{t.contains(99)}")
"#,
        "true\nfalse",
    );
}

#[test]
fn test_tuple_index_of() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.index_of(20)}")
println(f"{t.index_of(99)}")
"#,
        "1\n-1",
    );
}

#[test]
fn test_tuple_index_of_first_element() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.index_of(10)}")
"#,
        "0",
    );
}

#[test]
fn test_tuple_index_of_last_element() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (10, 20, 30)
println(f"{t.index_of(30)}")
"#,
        "2",
    );
}

#[test]
fn test_tuple_string_queryable_methods() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = ("hello", "world", "foo")
let first = t.first() ?? "none"
let last = t.last() ?? "none"
println(first)
println(last)
let c_yes = t.contains("world")
let c_no = t.contains("zzz")
println(f"{c_yes}")
println(f"{c_no}")
let i_first = t.index_of("hello")
let i_last = t.index_of("foo")
let i_missing = t.index_of("zzz")
println(f"{i_first}")
println(f"{i_last}")
println(f"{i_missing}")
println(f"{t.is_empty()}")
"#,
        "hello\nfoo\ntrue\nfalse\n0\n2\n-1\nfalse",
    );
}
