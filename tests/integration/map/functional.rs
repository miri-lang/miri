// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_filter_by_value() {
    assert_runs_with_output(
        "
use system.collections.map

let m = Map({\"a\": 1, \"b\": 2, \"c\": 3})
let filtered = m.filter(fn(k String, v int) bool: v > 1)
println(f\"{filtered.length()}\")
",
        "2",
    );
}

#[test]
fn map_map_transforms_values() {
    assert_runs_with_output(
        "
use system.collections.map

let m = Map({\"a\": 1, \"b\": 2, \"c\": 3})
let doubled = m.map(fn(k String, v int) int: v * 2)
let total = doubled.reduce(0, fn(acc int, k String, v int) int: acc + v)
println(f\"{total}\")
",
        "12",
    );
}

#[test]
fn map_reduce_sums_values() {
    assert_runs_with_output(
        "
use system.collections.map

let m = Map({\"a\": 1, \"b\": 2, \"c\": 3})
let total = m.reduce(0, fn(acc int, k String, v int) int: acc + v)
println(f\"{total}\")
",
        "6",
    );
}

/// A program struct named like the map's key parameter is the value type the
/// lambdas `reduce` and `filter` take: each lambda is handed the `String` key
/// and the struct value, every field at its own width.
#[test]
fn reduce_and_filter_over_a_struct_named_like_the_key_parameter() {
    assert_heap_guard_output(
        r#"
use system.collections.map

struct K
    a int
    b float

fn main()
    var m = Map<String, K>()
    m.set("x", K(a: 1, b: 2.5))
    m.set("yy", K(a: 2, b: 0.25))
    let total = m.reduce(K(a: 0, b: 0.0), fn(acc K, k String, v K) K: K(a: acc.a + v.a + k.length(), b: acc.b + v.b))
    let kept = m.filter(fn(k String, v K) bool: k == "x" and v.a == 1)
    println(f"{total.a} {total.b} {kept.length()}")
"#,
        "6 2.75 1",
    );
}

/// A program class with a drop hook named like the map's key parameter is
/// the value type `reduce`'s lambda takes and returns, each value released
/// once.
#[test]
fn reduce_over_a_resource_class_named_like_the_key_parameter() {
    assert_heap_guard_output(
        r#"
use system.collections.map

class K
    var n int
    fn init(n int)
        self.n = n
    fn drop(self)
        println(f"drop {self.n}")

fn main()
    var m = Map<String, K>()
    m.set("abc", K(3))
    let total = m.reduce(K(0), fn(acc K, k String, v K) K: K(acc.n + v.n + k.length()))
    println(f"{m.length()} {total.n}")
"#,
        "drop 0\n1 6\ndrop 6\ndrop 3",
    );
}
