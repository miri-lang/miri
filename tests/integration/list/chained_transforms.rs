// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Chaining one collection transform onto another's result, over an element
//! type the runtime reference-counts.
//!
//! The intermediate collection is a temporary no local binds, so it is released
//! as soon as the second call returns. Every element it holds has to belong to
//! it by then: a body that filled it while treating the element type as
//! unmanaged hands back a collection of borrowed pointers, and releasing it
//! frees elements the source collection still holds.
//!
//! Every test therefore reads the source collection *after* the chain has been
//! released. Without that read the defect is invisible — nothing looks at the
//! freed elements, and the program prints the right answer over a corrupted
//! heap. Binding the intermediate to a name does not remove the over-release
//! either, only defer it to the end of the scope, where whether anything
//! notices comes down to the allocator.
//!
//! `sum` is the one method covered over `int` instead: its contract is a
//! numeric element (`acc = acc + element`), and no reference-counted type is
//! numeric.

use super::utils::*;

/// Three words built at runtime, so none of them is an immortal string literal
/// whose release is a no-op, and `take(2)` leaves one element behind.
const WORDS: &str = "
use system.collections.list
use system.collections.transformable
use system.collections.sequenced
use system.collections.foldable

fn main()
    var rows = List<String>()
    rows.push(\"b\" + \"b\")
    rows.push(\"a\" + \"a\")
    rows.push(\"c\" + \"c\")
";

/// `body` is appended to a program that leaves `rows` holding `bb`, `aa`, `cc`.
fn words_program(body: &str) -> String {
    format!("{}{}", WORDS, body)
}

/// The chain, then the source: `rows` still owns its three words after the
/// intermediate and the result have both been released.
fn chain_then_read_source(chain: &str, render: &str) -> String {
    words_program(&format!(
        "    let out = {chain}
    println(\",\".join(rows))
    {render}
"
    ))
}

#[test]
fn chained_transform_over_struct_elements_keeps_every_field() {
    assert_repeated_runs_have_output(
        r#"
use system.collections.list
use system.collections.sequenced

struct W
    word String
    count int

fn main()
    var rows = List<W>()
    rows.push(W("the", 3))
    rows.push(W("fox", 2))
    rows.push(W("dog", 1))
    let cmp = fn(a W, b W) int
        b.count - a.count
    let top = rows.sorted_by(cmp).take(2)
    for r in top
        println(f"{r.word}:{r.count}")
"#,
        "the:3\nfox:2",
        50,
    );
}

/// The same program under the heap guard, which reports a read of a freed block
/// even on the runs where the allocator has not yet handed it out again.
#[test]
fn chained_transform_over_struct_elements_touches_no_freed_block() {
    assert_heap_guard_ok(
        r#"
use system.collections.list
use system.collections.sequenced

struct W
    word String
    count int

fn main()
    var rows = List<W>()
    rows.push(W("the", 3))
    rows.push(W("fox", 2))
    rows.push(W("dog", 1))
    let cmp = fn(a W, b W) int
        b.count - a.count
    let top = rows.sorted_by(cmp).take(2)
    for r in top
        println(f"{r.word}:{r.count}")
    for r in rows
        println(f"{r.word}:{r.count}")
"#,
    );
}

#[test]
fn map_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source(
            "rows.reversed().map(fn(s String) String: s)",
            "println(\",\".join(out))",
        ),
        "bb,aa,cc\ncc,aa,bb",
    );
}

#[test]
fn filter_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source(
            "rows.reversed().filter(fn(s String) bool: s != \"aa\")",
            "println(\",\".join(out))",
        ),
        "bb,aa,cc\ncc,bb",
    );
}

#[test]
fn flat_map_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source(
            "rows.reversed().flat_map(fn(s String) [String]: List([s]))",
            "println(\",\".join(out))",
        ),
        "bb,aa,cc\ncc,aa,bb",
    );
}

#[test]
fn reduce_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source(
            "rows.reversed().reduce(\"\", fn(a String, b String) String: a + b)",
            "println(out)",
        ),
        "bb,aa,cc\nccaabb",
    );
}

#[test]
fn any_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source(
            "rows.reversed().any(fn(s String) bool: s == \"aa\")",
            "let miss = rows.reversed().any(fn(s String) bool: s == \"zz\")
    println(f\"{out} {miss}\")",
        ),
        "bb,aa,cc\ntrue false",
    );
}

#[test]
fn all_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source(
            "rows.reversed().all(fn(s String) bool: s != \"zz\")",
            "let not_every = rows.reversed().all(fn(s String) bool: s == \"aa\")
    println(f\"{out} {not_every}\")",
        ),
        "bb,aa,cc\ntrue false",
    );
}

#[test]
fn count_where_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source(
            "rows.reversed().count_where(fn(s String) bool: s != \"aa\")",
            "println(f\"{out}\")",
        ),
        "bb,aa,cc\n2",
    );
}

/// `min` and `max` over a one-element chain do not reach the comparison the
/// shared generic body lowers as a pointer compare, so what they assert is that
/// the element survives the chain — the value handed back is the element the
/// chain selected, not the outcome of an ordering.
#[test]
fn min_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source("rows.reversed().take(1).min() ?? \"none\"", "println(out)"),
        "bb,aa,cc\ncc",
    );
}

#[test]
fn max_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source("rows.reversed().take(1).max() ?? \"none\"", "println(out)"),
        "bb,aa,cc\ncc",
    );
}

/// `sum` needs a numeric element, so the chain covering it carries `int`. It
/// reaches the same lowering the managed cases do; only the element differs.
#[test]
fn sum_chained_onto_a_transform_result() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.sequenced
use system.collections.foldable

fn main()
    let l = List([1, 2, 3])
    println(f\"{l.reversed().sum() ?? 0}\")
",
        "6",
    );
}

#[test]
fn take_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source("rows.reversed().take(2)", "println(\",\".join(out))"),
        "bb,aa,cc\ncc,aa",
    );
}

#[test]
fn skip_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source("rows.reversed().skip(1)", "println(\",\".join(out))"),
        "bb,aa,cc\naa,bb",
    );
}

#[test]
fn sorted_by_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source(
            "rows.reversed().sorted_by(fn(a String, b String) int: a.compare(b))",
            "println(\",\".join(out))",
        ),
        "bb,aa,cc\naa,bb,cc",
    );
}

#[test]
fn unique_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source(
            "rows.reversed().flat_map(fn(s String) [String]: List([s, s])).unique()",
            "println(\",\".join(out))",
        ),
        "bb,aa,cc\ncc,aa,bb",
    );
}

#[test]
fn reversed_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source("rows.reversed().reversed()", "println(\",\".join(out))"),
        "bb,aa,cc\nbb,aa,cc",
    );
}

#[test]
fn zip_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source(
            "rows.reversed().zip(rows)",
            "for pair in out
        println(f\"{pair.0}-{pair.1}\")",
        ),
        "bb,aa,cc\ncc-bb\naa-aa\nbb-cc",
    );
}

#[test]
fn enumerate_chained_onto_a_transform_result() {
    assert_runs_with_output(
        &chain_then_read_source(
            "rows.reversed().enumerate()",
            "for pair in out
        println(f\"{pair.0}:{pair.1}\")",
        ),
        "bb,aa,cc\n0:cc\n1:aa\n2:bb",
    );
}
