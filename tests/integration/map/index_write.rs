// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_index_write() {
    assert_runs_with_output(
        r#"

var m = {"a": 1}
m["a"] = 10
let v = m["a"]
println(f"{v}")
"#,
        "10",
    );
}

#[test]
fn map_index_write_new_key() {
    assert_runs_with_output(
        r#"

var m = {"a": 1}
m["b"] = 2
let v = m["b"]
println(f"{v}")
"#,
        "2",
    );
}

/// Writing through an index must take an exclusive copy first, exactly as the
/// `set` method does. Without the check the write lands in the shared buffer
/// and the original binding sees the new entry.
#[test]
fn map_index_write_copies_before_mutating_an_alias() {
    assert_runs_with_output(
        r#"
use system.collections.map

var original = Map<String, int>()
original["x"] = 1
var alias = original
alias["y"] = 2
println(f"{original.length()}")
println(f"{alias.length()}")
"#,
        "1\n2",
    );
}

/// The copy must only happen when the buffer is actually shared — an unaliased
/// map still mutates in place.
#[test]
fn map_index_write_still_mutates_when_unshared() {
    assert_runs_with_output(
        r#"
use system.collections.map

var entries = Map<String, int>()
entries["x"] = 1
entries["y"] = 2
println(f"{entries.length()}")
"#,
        "2",
    );
}
