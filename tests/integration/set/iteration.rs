// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_set_for_loop() {
    assert_runs_with_output(
        r#"
use system.collections.set

// Order in a Set is not strictly guaranteed by definition,
// but the current implementation likely preserves insertion order
// sequentially. Let's sum them to be safe from ordering issues.
let s = {10, 20, 30}
var sum = 0
for x in s
    sum = sum + x
println(f"{sum}")
"#,
        "60",
    );
}

#[test]
fn test_set_empty_iteration() {
    assert_runs_with_output(
        r#"
use system.collections.set
var s = {1, 2, 3}
s.clear()
var count = 0
for x in s
    count = count + 1
println(f"{count}")
"#,
        "0",
    );
}

/// Every read of a set's element hands the reader a reference of its own, so a
/// loop that releases its binding each pass leaves the set holding what it held.
/// The strings are built at runtime — a pooled literal is immortal and would
/// survive the over-release this guards against — and the set is read three
/// times over, because the first pass is the one that frees and the later ones
/// are the ones that notice.
#[test]
fn set_of_runtime_strings_survives_repeated_reads() {
    assert_heap_guard_output(
        r#"
use system.collections.set

fn main()
    var s = Set<String>()
    s.add("a" + "a")
    s.add("b" + "bb")
    var total = 0
    for x in s
        total += x.length()
    for x in s
        total += x.length()
    let first = s.element_at(0)
    let second = s.element_at(1)
    println(f"{total} {first.length() + second.length()}")
"#,
        "10 5",
    );
}
