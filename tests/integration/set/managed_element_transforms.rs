// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `filter`, `map` and `reduce` over a `Set` whose elements the runtime
//! reference-counts.
//!
//! These are methods `Set` declares itself, and each builds its result out of
//! what its function argument hands back and what `element_at` reads. A body
//! that treats the element type as unmanaged gets both wrong: the strings `map`
//! and `reduce` receive from the function are never released, and the set
//! `filter` fills holds pointers it does not own, so releasing the source frees
//! what the result still points at.
//!
//! Every element is built at runtime, so none is an immortal string literal
//! whose release is a no-op, and every test runs under the heap guard, which
//! reports both the read of a freed block and an allocation left behind. The
//! iteration order of a set is unspecified, so what is printed never depends on
//! it: membership, counts and summed lengths.

use super::utils::*;

/// A program whose `main` is `body`, with `make` building a set of `aa`, `bb`
/// and `ccc` at runtime, `total_length` summing the lengths of a set's elements
/// — a read of every element, in whatever order the set keeps them — and
/// `count_of` finding a word by comparing content. `contains` is not used for
/// membership: it compares a set's elements by address, so it never finds a
/// string built at runtime.
fn words_program(body: &str) -> String {
    format!(
        "
use system.collections.set

fn make() {{String}}
    var s = Set<String>()
    s.add(\"a\" + \"a\")
    s.add(\"b\" + \"b\")
    s.add(\"c\" + \"cc\")
    return s

fn total_length(s {{String}}) int
    var total = 0
    for x in s
        total += x.length()
    return total

fn count_of(s {{String}}, word String) int
    var n = 0
    for x in s
        if x == word
            n += 1
    return n

fn main()
{body}"
    )
}

#[test]
fn set_filter_keeps_matching_strings_and_leaves_the_source_whole() {
    assert_heap_guard_output(
        &words_program(
            "    let s = make()
    let kept = s.filter(fn(x String) bool: x != \"bb\")
    let has_aa = count_of(kept, \"aa\") == 1
    let has_bb = count_of(kept, \"bb\") == 1
    println(f\"{kept.length()} {has_aa} {has_bb} {total_length(kept)}\")
    let source_has_bb = count_of(s, \"bb\") == 1
    println(f\"{s.length()} {source_has_bb} {total_length(s)}\")
",
        ),
        "2 true false 5\n3 true 7",
    );
}

#[test]
fn set_filter_result_outlives_a_temporary_source() {
    assert_heap_guard_output(
        &words_program(
            "    let kept = make().filter(fn(x String) bool: x.length() == 2)
    let has_aa = count_of(kept, \"aa\") == 1
    println(f\"{kept.length()} {has_aa} {total_length(kept)}\")
",
        ),
        "2 true 4",
    );
}

#[test]
fn set_map_owns_the_strings_the_function_returns() {
    assert_heap_guard_output(
        &words_program(
            "    let s = make()
    let loud = s.map(fn(x String) String: x + \"!\")
    let has_loud = count_of(loud, \"ccc!\") == 1
    println(f\"{loud.length()} {has_loud} {total_length(loud)}\")
    let source_has_ccc = count_of(s, \"ccc\") == 1
    println(f\"{s.length()} {source_has_ccc} {total_length(s)}\")
",
        ),
        "3 true 10\n3 true 7",
    );
}

#[test]
fn set_map_returning_the_element_itself_outlives_a_temporary_source() {
    assert_heap_guard_output(
        &words_program(
            "    let same = make().map(fn(x String) String: x)
    let has_bb = count_of(same, \"bb\") == 1
    println(f\"{same.length()} {has_bb} {total_length(same)}\")
",
        ),
        "3 true 7",
    );
}

#[test]
fn set_reduce_releases_every_intermediate_accumulator() {
    assert_heap_guard_output(
        &words_program(
            "    let s = make()
    let joined = s.reduce(\"<\" + \">\", fn(acc String, x String) String: acc + x)
    println(f\"{joined.length()} {s.length()} {total_length(s)}\")
",
        ),
        "9 3 7",
    );
}

#[test]
fn set_transforms_over_an_empty_set_hand_back_empty_results() {
    assert_heap_guard_output(
        "
use system.collections.set

fn main()
    let s = Set<String>()
    let kept = s.filter(fn(x String) bool: true)
    let loud = s.map(fn(x String) String: x + \"!\")
    let joined = s.reduce(\"in\" + \"it\", fn(acc String, x String) String: acc + x)
    println(f\"{kept.length()} {loud.length()} {joined}\")
",
        "0 0 init",
    );
}

#[test]
fn set_transforms_over_a_single_string() {
    assert_heap_guard_output(
        &words_program(
            "    var s = Set<String>()
    s.add(\"o\" + \"ne\")
    let kept = s.filter(fn(x String) bool: x == \"one\")
    let dropped = s.filter(fn(x String) bool: x != \"one\")
    let loud = s.map(fn(x String) String: x + \"!\")
    let joined = s.reduce(\"\", fn(acc String, x String) String: acc + x + x)
    let has_loud = count_of(loud, \"one!\") == 1
    let source_has_one = count_of(s, \"one\") == 1
    println(f\"{kept.length()} {dropped.length()} {has_loud} {joined}\")
    println(f\"{source_has_one}\")
",
        ),
        "1 0 true oneone\ntrue",
    );
}
