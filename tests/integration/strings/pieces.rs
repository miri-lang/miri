// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `s.pieces(sep)` and `s.lines()` hand out the pieces of a string one at a
//! time, from a cursor over the source, instead of building a list of all of
//! them the way `split` does.

use super::utils::*;

/// A program that checks `s.pieces(sep)` against `s.split(sep)` for every
/// case: the same count (asked twice), the same pieces walked in order by
/// `for`, and the same pieces read back to front through `element_at`.
fn agreement_program(cases: &[(&str, &str)]) -> String {
    let checks: String = cases
        .iter()
        .map(|(text, sep)| {
            format!(
                "if not agree(\"{text}\", \"{sep}\"): println(\"differs: [{text}] by [{sep}]\")\n"
            )
        })
        .collect();
    format!(
        r#"
use system.io
use system.collections.list

fn agree(text String, sep String) bool
    let eager = text.split(sep)
    let lazy = text.pieces(sep)
    if lazy.length() != eager.length() or lazy.length() != eager.length(): return false
    var i = 0
    for piece in lazy
        if piece != eager[i]: return false
        i += 1
    var j = eager.length() - 1
    while j >= 0
        if lazy.element_at(j) != eager[j]: return false
        j -= 1
    i == eager.length()

{checks}println("done")
"#
    )
}

#[test]
fn test_pieces_agree_with_split_on_every_shape() {
    let cases = [
        ("", ","),
        ("abc", ","),
        ("a,b,", ","),
        (",a,b", ","),
        ("a,,b", ","),
        (",,", ","),
        ("a::b::::c::", "::"),
        ("::", "::"),
        ("héllo wörld ñ", " "),
        ("日本語,中文,", ","),
        ("aébéc", "é"),
        ("héy", ""),
        ("", ""),
    ];
    assert_runs_with_output(&agreement_program(&cases), "done");
}

#[test]
fn test_lines_split_on_newlines_and_drop_one_trailing_newline() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn show(text String) String
    let lines = text.lines()
    var seen = List<String>()
    for line in lines
        seen.push(f"[{line}]")
    let joined = "".join(seen)
    f"{lines.length()}:{joined}"

println(show(""))
println(show("a"))
println(show("a\n"))
println(show("a\nb"))
println(show("a\r\nb\r\n"))
println(show("\n"))
println(show("\n\n"))
println(show("a\n\nb\n"))
println(show("x\ry"))
println(show("a\r"))
println(show("é\nü\r\n日本"))
"#,
        "0:\n1:[a]\n1:[a]\n2:[a][b]\n2:[a][b]\n1:[]\n2:[][]\n3:[a][][b]\n1:[x\ry]\n1:[a\r]\n3:[é][ü][日本]",
    );
}

#[test]
fn test_pieces_and_lines_read_out_of_order() {
    assert_runs_with_output(
        r#"
use system.io

let lines = "one\r\ntwo\nthree\nfour\n".lines()
let last = lines.length() - 1
println(lines.element_at(1))
println(lines.element_at(last))
println(lines.element_at(0))
println(lines.element_at(2))
println(lines.element_at(2))
let p = "a--bb--ccc".pieces("--")
println(p.element_at(2))
println(p.element_at(0))
println(p.element_at(1))
println(f"{p.length()} {p.length()} {last}")
"#,
        "two\nfour\none\nthree\nthree\nccc\na\nbb\n3 3 3",
    );
}

#[test]
fn test_a_piece_past_the_end_is_an_index_out_of_bounds_error() {
    assert_runtime_error(
        r#"
use system.io

let p = "a,b".pieces(",")
var i = 2
println(p.element_at(i))
"#,
        "index out of bounds: the len is 2 but the index is 2",
    );
}

#[test]
fn test_a_negative_line_index_is_an_index_out_of_bounds_error() {
    assert_runtime_error(
        r#"
use system.io

let lines = "a\nb\n".lines()
let i = 0 - 1
println(lines.element_at(i))
"#,
        "index out of bounds: the len is 2 but the index is -1",
    );
}

#[test]
fn test_an_empty_separator_yields_characters_as_split_does() {
    assert_runs_with_output(
        r#"
use system.io

for ch in "héy".pieces("")
    println(f"[{ch}]")
let lazy = "".pieces("").length()
let eager = "".split("").length()
println(f"{lazy} {eager}")
"#,
        "[h]\n[é]\n[y]\n0 0",
    );
}

#[test]
fn test_pieces_outlive_the_iterable_and_the_iterable_outlives_its_source() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn kept_pieces() [String]
    var text = "alpha,beta" * 2
    let pieces = text.pieces(",")
    text = "replaced"
    var kept = List<String>()
    for piece in pieces
        kept.push(piece)
    kept

fn last_line() String
    let lines = ("x\n" * 3 + "end\n").lines()
    lines.element_at(lines.length() - 1)

let kept = kept_pieces()
println(",".join(kept))
println(last_line())
"#,
        "alpha,betaalpha,beta\nend",
    );
}

/// The raw counts a program's allocations reach, as `MIRI_ALLOC_COUNT`
/// reports them, with the leak check on.
fn line_walk_allocations(lines: usize) -> AllocationBreakdown {
    let code = format!(
        r#"
use system.io

let text = "x\n" * {lines}
var bytes = 0
for line in text.lines()
    bytes += line.size()
println(f"{{bytes}}")
"#
    );
    parse_alloc_count_breakdown(&code)
}

#[test]
fn test_walking_the_lines_allocates_one_string_per_line_and_no_list() {
    let few = line_walk_allocations(10_000);
    let many = line_walk_allocations(20_000);
    let per_line = many.rc - few.rc;
    assert!(
        (9_990..=10_010).contains(&per_line),
        "10 000 more lines made {per_line} more counted values"
    );
    assert_eq!(
        (few.buffers, many.buffers),
        (0, 0),
        "walking the lines allocated a collection buffer"
    );
    assert_eq!(few.raw, many.raw, "walking more lines allocated raw memory");
}

#[test]
fn test_walking_lines_passes_the_heap_guard() {
    assert_heap_guard_output(
        r#"
use system.io

let text = "row 1\r\nrow 2\nrow 3\n" * 1000
var count = 0
var bytes = 0
for line in text.lines()
    count += 1
    bytes += line.size()
println(f"{count} {bytes}")
"#,
        "3000 15000",
    );
}
