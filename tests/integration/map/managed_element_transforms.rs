// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `filter`, `map` and `reduce` over a `Map` whose keys and values the runtime
//! reference-counts.
//!
//! These are methods `Map` declares itself, and each builds its result out of
//! what its function argument hands back and what `element_at` and `value_at`
//! read. A body that treats the key and value types as unmanaged gets both
//! wrong: the strings `map` and `reduce` receive from the function are never
//! released, and the map `filter` fills holds pointers it does not own, so
//! releasing the source frees what the result still points at.
//!
//! Every key and value is built at runtime, so none is an immortal string
//! literal whose release is a no-op, and every test runs under the heap guard,
//! which reports both the read of a freed block and an allocation left behind.
//! The iteration order of a map is unspecified, so what is printed never depends
//! on it: lookups, counts and summed lengths.

use super::utils::*;

/// A program whose `main` is `body`, with `make` building the map
/// `aa: x, bb: yy, ccc: zzz` at runtime and `total_length` summing the lengths
/// of every key and value — a read of every entry, in whatever order the map
/// keeps them.
fn entries_program(body: &str) -> String {
    format!(
        "
use system.collections.map

fn make() {{String: String}}
    var m = Map<String, String>()
    m.set(\"a\" + \"a\", \"x\" + \"\")
    m.set(\"b\" + \"b\", \"y\" + \"y\")
    m.set(\"c\" + \"cc\", \"z\" + \"zz\")
    return m

fn total_length(m {{String: String}}) int
    var total = 0
    var i = 0
    while i < m.length()
        total += m.element_at(i).length() + m.value_at(i).length()
        i += 1
    return total

fn main()
{body}"
    )
}

#[test]
fn map_filter_keeps_matching_entries_and_leaves_the_source_whole() {
    assert_heap_guard_output(
        &entries_program(
            "    let m = make()
    let kept = m.filter(fn(k String, v String) bool: k != \"bb\")
    let ccc = kept.get(\"ccc\") ?? \"none\"
    let has_bb = kept.contains_key(\"bb\")
    println(f\"{kept.length()} {ccc} {has_bb} {total_length(kept)}\")
    let source_bb = m.get(\"bb\") ?? \"none\"
    println(f\"{m.length()} {source_bb} {total_length(m)}\")
",
        ),
        "2 zzz false 9\n3 yy 13",
    );
}

#[test]
fn map_filter_result_outlives_a_temporary_source() {
    assert_heap_guard_output(
        &entries_program(
            "    let kept = make().filter(fn(k String, v String) bool: v.length() > 1)
    let bb = kept.get(\"bb\") ?? \"none\"
    println(f\"{kept.length()} {bb} {total_length(kept)}\")
",
        ),
        "2 yy 10",
    );
}

#[test]
fn map_map_owns_the_values_the_function_returns() {
    assert_heap_guard_output(
        &entries_program(
            "    let m = make()
    let joined = m.map(fn(k String, v String) String: k + \"=\" + v)
    let ccc = joined.get(\"ccc\") ?? \"none\"
    println(f\"{joined.length()} {ccc} {total_length(joined)}\")
    let source_ccc = m.get(\"ccc\") ?? \"none\"
    println(f\"{m.length()} {source_ccc} {total_length(m)}\")
",
        ),
        "3 ccc=zzz 23\n3 zzz 13",
    );
}

#[test]
fn map_map_returning_the_value_itself_outlives_a_temporary_source() {
    assert_heap_guard_output(
        &entries_program(
            "    let same = make().map(fn(k String, v String) String: v)
    let aa = same.get(\"aa\") ?? \"none\"
    println(f\"{same.length()} {aa} {total_length(same)}\")
",
        ),
        "3 x 13",
    );
}

#[test]
fn map_reduce_releases_every_intermediate_accumulator() {
    assert_heap_guard_output(
        &entries_program(
            "    let m = make()
    let joined = m.reduce(\"<\" + \">\", fn(acc String, k String, v String) String: acc + k + v)
    println(f\"{joined.length()} {m.length()} {total_length(m)}\")
",
        ),
        "15 3 13",
    );
}

#[test]
fn map_transforms_over_an_empty_map_hand_back_empty_results() {
    assert_heap_guard_output(
        "
use system.collections.map

fn main()
    let m = Map<String, String>()
    let kept = m.filter(fn(k String, v String) bool: true)
    let joined = m.map(fn(k String, v String) String: k + v)
    let folded = m.reduce(\"in\" + \"it\", fn(acc String, k String, v String) String: acc + k)
    println(f\"{kept.length()} {joined.length()} {folded}\")
",
        "0 0 init",
    );
}

#[test]
fn map_transforms_over_a_single_entry() {
    assert_heap_guard_output(
        "
use system.collections.map

fn main()
    var m = Map<String, String>()
    m.set(\"o\" + \"ne\", \"1\" + \"1\")
    let kept = m.filter(fn(k String, v String) bool: k == \"one\")
    let dropped = m.filter(fn(k String, v String) bool: v == \"no\")
    let joined = m.map(fn(k String, v String) String: v + k)
    let folded = m.reduce(\"\", fn(acc String, k String, v String) String: acc + k + v)
    let value = joined.get(\"one\") ?? \"none\"
    let source_value = m.get(\"one\") ?? \"none\"
    println(f\"{kept.length()} {dropped.length()} {value} {folded}\")
    println(source_value)
",
        "1 0 11one one11\n11",
    );
}
