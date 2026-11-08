// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_tuple_contains_duplicates() {
    assert_runs_with_output(
        r#"

let t = (10, 20, 10, 30)
println(f"{t.contains(10)}")
println(f"{t.index_of(10) ?? -1}")
"#,
        "true\n0",
    );
}

#[test]
fn test_tuple_large() {
    assert_runs_with_output(
        r#"

let t = (1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
println(f"{t.length()}")
println(f"{t.first() ?? -1}")
println(f"{t.last() ?? -1}")
println(f"{t.contains(5)}")
println(f"{t.index_of(10) ?? -1}")
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

let t1 = (1, 2)
let t2 = (1, 2)
let t3 = (2, 1)
println(f"{t1 == t2}")
println(f"{t1 == t3}")
"#,
        "true\nfalse",
    );
}

/// A wide tuple literal holding every kind of member: each member is read
/// against the tuple's declared type, which is read once for the literal,
/// and every member keeps its own value and width.
#[test]
fn a_wide_tuple_of_mixed_members_keeps_every_member() {
    let members: Vec<String> = (0..600)
        .map(|i| match i % 4 {
            0 => format!("{i}"),
            1 => format!("Some({i})"),
            2 => format!("\"s{i}\" + \"!\""),
            _ => format!("{i}.5"),
        })
        .collect();
    let code = format!(
        "fn main()\n    let t = ({})\n    println(f\"{{t.0}} {{t.1}} {{t.2}} {{t.3}} {{t.597}} {{t.598}} {{t.599}}\")\n",
        members.join(", ")
    );
    assert_heap_guard_output(&code, "0 Some(1) s2! 3.5 Some(597) s598! 599.5");
}
