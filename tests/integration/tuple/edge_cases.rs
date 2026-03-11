// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_tuple_contains_duplicates() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (10, 20, 10, 30)
println(f"{t.contains(10)}")
println(f"{t.index_of(10)}")
"#,
        "true\n0",
    );
}

#[test]
fn test_tuple_large() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.tuple

let t = (1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
println(f"{t.length()}")
println(f"{t.first() ?? -1}")
println(f"{t.last() ?? -1}")
println(f"{t.contains(5)}")
println(f"{t.index_of(10)}")
"#,
        "10\n1\n10\ntrue\n9",
    );
}

#[test]
fn test_tuple_empty() {
    // Empty tuple () is Tuple() (no generic T), so Tuple<T> methods don't resolve.
    // We can only test that an empty tuple compiles and can be assigned.
    assert_runs(
        r#"
let t = ()
"#,
    );
}

#[test]
fn test_tuple_equality() {
    assert_runs_with_output(
        r#"
use system.io

let t1 = (1, 2)
let t2 = (1, 2)
let t3 = (2, 1)
println(f"{t1 == t2}")
println(f"{t1 == t3}")
"#,
        "true\nfalse",
    );
}
