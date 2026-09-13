// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Chaining one `Array` transform onto another's result, over an element type
//! the runtime reference-counts.
//!
//! An `Array` carries its size as a second generic argument, and that argument
//! is a value rather than a type. A per-instantiation body is named after every
//! argument, so the size has to contribute a token of its own: without one the
//! receiver cannot be spelled at all and every call falls back to the shared
//! generic body, which stores each element without taking a reference while the
//! call site releases every one of them.
//!
//! The transforms an `Array` inherits return a `List`, a type such a program
//! never writes down. That instantiation still needs its own body for the same
//! reason, so the registry has to hold the types a program reaches through a
//! return as well as the ones it spells.

use super::utils::*;

/// A struct whose first field is heap-allocated, so an element handed back
/// without a reference is a freed block by the time it is read.
const WORDS: &str = r#"
use system.collections.array
use system.collections.transformable
use system.collections.sequenced

struct W
    word String
    count int
"#;

fn words_program(body: &str) -> String {
    format!("{WORDS}{body}")
}

#[test]
fn array_chained_transform_over_struct_elements_keeps_every_field() {
    assert_repeated_runs_have_output(
        &words_program(
            r#"
fn main()
    let rows = [W("th" + "e", 3), W("fo" + "x", 2), W("do" + "g", 1)]
    let top = rows.reversed().take(2)
    for r in top
        println(f"{r.word}:{r.count}")
"#,
        ),
        "dog:1\nfox:2",
        50,
    );
}

/// The same chain under the heap guard, which reports a read of a freed block
/// even on the runs where the allocator has not yet handed it out again. The
/// source array is read after the chain has been released, so an element the
/// chain over-released is touched while the array still holds it.
#[test]
fn array_chained_transform_touches_no_freed_block() {
    assert_heap_guard_ok(&words_program(
        r#"
fn main()
    let rows = [W("th" + "e", 3), W("fo" + "x", 2), W("do" + "g", 1)]
    let top = rows.reversed().take(2)
    for r in top
        println(f"{r.word}:{r.count}")
    for r in rows
        println(f"{r.word}:{r.count}")
"#,
    ));
}

/// Two arrays of the same element type and different sizes in one program. The
/// size contributes to the mangled name, so each gets its own body; sharing one
/// would compile the second against the first's instantiation.
#[test]
fn arrays_of_two_sizes_each_transform_correctly() {
    assert_runs_with_output(
        &words_program(
            r#"
fn main()
    let two = [W("a" + "a", 1), W("b" + "b", 2)]
    let three = [W("c" + "c", 3), W("d" + "d", 4), W("e" + "e", 5)]
    let from_two = two.reversed().take(1)
    let from_three = three.reversed().take(1)
    for r in from_two
        println(f"{r.word}:{r.count}")
    for r in from_three
        println(f"{r.word}:{r.count}")
    for r in two
        println(f"{r.word}:{r.count}")
    for r in three
        println(f"{r.word}:{r.count}")
"#,
        ),
        "bb:2\nee:5\naa:1\nbb:2\ncc:3\ndd:4\nee:5",
    );
}

/// `filter` inherited by an `Array` returns a `List`, which this program never
/// spells. Reading the source array afterwards proves the elements the filtered
/// list kept were retained rather than borrowed.
#[test]
fn array_filter_result_owns_the_elements_it_keeps() {
    assert_heap_guard_ok(&words_program(
        r#"
fn main()
    let rows = [W("th" + "e", 3), W("fo" + "x", 2), W("do" + "g", 1)]
    let kept = rows.filter(fn(w W) bool: w.count > 1)
    for r in kept
        println(f"{r.word}:{r.count}")
    for r in rows
        println(f"{r.word}:{r.count}")
"#,
    ));
}

/// An array of runtime-built strings, the simplest managed element there is.
#[test]
fn array_of_strings_survives_a_chained_transform() {
    assert_runs_with_output(
        r#"
use system.collections.array
use system.collections.transformable
use system.collections.sequenced

fn main()
    let rows = ["b" + "b", "a" + "a", "c" + "c"]
    let out = rows.reversed().take(2)
    println(",".join(out))
    for word in rows
        println(word)
"#,
        "cc,aa\nbb\naa\ncc",
    );
}

/// Ordering over an `Array` of runtime-built strings. `min` and `max` are
/// inherited defaults that compare elements, so before the size argument had a
/// token they ran in the shared body — where the element is an unresolved
/// parameter and the comparison falls back to comparing addresses.
#[test]
fn array_ordering_compares_string_content_not_addresses() {
    assert_runs_with_output(
        r#"
use system.collections.array
use system.collections.foldable

fn main()
    let fruit = ["pe" + "ar", "ap" + "ple", "ki" + "wi"]
    let lo = fruit.min() ?? "none"
    let hi = fruit.max() ?? "none"
    println(f"{lo} {hi}")
"#,
        "apple pear",
    );
}
