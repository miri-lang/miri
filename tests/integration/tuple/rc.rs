// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_tuple_alias_no_double_free() {
    assert_runs(
        r#"
let t1 = (1, 2, 3)
let t2 = t1
"#,
    );
}

#[test]
fn test_tuple_reassign() {
    assert_runs(
        r#"
var t = (1, 2, 3)
t = (4, 5, 6)
"#,
    );
}

#[test]
fn test_tuple_with_managed_types() {
    assert_runs(
        r#"
use system.collections.list

// If this crashes, there's a problem with tuple drop code.
// Tuples shouldn't leak memory (verified by leak sanitizer / Miri internal RC checks if any).
let t = (List([1, 2, 3]), "hello")
let l2 = t.0 // Increase RC
let s2 = t.1 // Increase RC
"#,
    );
}

#[test]
fn test_tuple_nested_managed_rc() {
    assert_runs(
        r#"
use system.collections.list
use system.collections.tuple

let t = (List([List([1])]), "outer")
let inner = t.0.element_at(0)
"#,
    );
}
