// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A short string keeps its bytes in the block that holds its header, so a
//! substring or a split piece costs one block and no separate data buffer.
//! Every reader and writer of a string sees the same text whatever its bytes
//! live in.

use super::utils::{
    assert_heap_guard_output, assert_runs_with_output, parse_alloc_count_breakdown,
};

/// Splitting a literal into short pieces allocates each piece's header and
/// bytes together, so no raw data buffer is made; the run also passes the
/// leak check, so every block is released.
#[test]
fn test_split_pieces_take_no_raw_allocation() {
    let breakdown = parse_alloc_count_breakdown(
        r#"
fn main()
    let parts = "a,b,c".split(",")
    println(parts[1])
"#,
    );
    assert_eq!(
        breakdown.raw, 0,
        "split pieces made raw allocations (rc={} buffers={} raw={})",
        breakdown.rc, breakdown.buffers, breakdown.raw
    );
}

/// Appending to a split piece moves its text into a buffer that can grow, and
/// loses none of it.
#[test]
fn test_appending_to_a_split_piece_keeps_its_text() {
    assert_runs_with_output(
        r#"
fn main()
    println("ab,x".split(",")[0] + "c")
    var piece = "ab,x".split(",")[0]
    piece += "c"
    println(piece)
"#,
        "abc\nabc",
    );
}

/// Appends that run far past the room a short piece was made with keep every
/// byte, the first ones included.
#[test]
fn test_repeated_appends_past_a_short_piece_keep_the_text() {
    assert_runs_with_output(
        r#"
fn main()
    var piece = "héllo,world".split(",")[0]
    var i = 0
    while i < 100
        piece += "-é"
        i += 1
    println(f"{piece.length()} {piece.substring(0, 7)} {piece.substring(203, 205)}")
"#,
        "205 héllo-é -é",
    );
}

/// A literal is never released, and a variable holding one can still be
/// appended to: the literal itself keeps its text.
#[test]
fn test_appending_to_a_variable_holding_a_literal_leaves_the_literal() {
    assert_runs_with_output(
        r#"
fn greeting() String
    "hello"

fn main()
    var s = greeting()
    s += " world"
    var i = 0
    while i < 3
        s += "!"
        i += 1
    println(s)
    println(greeting())
"#,
        "hello world!!!\nhello",
    );
}

/// Short pieces reused for new text, again and again, report the characters
/// of the text they now hold, not the text a freed piece held at the same
/// address.
#[test]
fn test_reused_short_pieces_count_their_own_characters() {
    assert_runs_with_output(
        r#"
fn main()
    var i = 0
    var out = ""
    while i < 4
        let first = "héllo,x".split(",")[0]
        out += f"{first.length()}{first.element_at(1)} "
        let second = "日本,x".split(",")[0]
        out += f"{second.length()}{second.element_at(1)} "
        i += 1
    println(out.trim())
"#,
        "5é 2本 5é 2本 5é 2本 5é 2本",
    );
}

const MAP_KEYS_FROM_PIECES: &str = r#"
use system.collections.map

fn main()
    var counts = Map<String, int>()
    for word in "apple,pear,apple,fig,pear,apple".split(",")
        counts[word] = (counts.get(word) ?? 0) + 1
    let key = "fig,x".split(",")[0]
    let apples = counts["apple"]
    let pears = counts["pear"]
    let figs = counts.get(key) ?? 0
    println(f"{apples} {pears} {figs} {counts.length()}")
    let same_hash = key.hash() == "fig".hash()
    println(f"{same_hash} {key == 'fig'} [{key}]")
"#;

/// Split pieces used as map keys are found by a literal of the same text,
/// hash and compare as that literal, and print as their text.
#[test]
fn test_split_pieces_work_as_map_keys() {
    assert_runs_with_output(MAP_KEYS_FROM_PIECES, "3 2 1 3\ntrue true [fig]");
}

/// The same map program under the heap guard: no block is read after release
/// and every block is released at the size it was allocated with.
#[test]
fn test_split_pieces_as_map_keys_under_the_heap_guard() {
    assert_heap_guard_output(MAP_KEYS_FROM_PIECES, "3 2 1 3\ntrue true [fig]");
}

const LONG_TEXT: &str =
    "  0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz-é  ";

/// A substring or trim too long to share its header's block still holds the
/// whole text.
#[test]
fn test_long_substring_and_trim_keep_the_text() {
    let code = format!(
        r#"
fn main()
    let text = "{LONG_TEXT}"
    let middle = text.substring(2, 256)
    let trimmed = text.trim()
    println(f"{{middle.length()}} {{trimmed.length()}} {{trimmed.substring(252, 254)}} {{middle == trimmed}}")
"#
    );
    assert_runs_with_output(&code, "254 254 -é true");
}

/// Removing split pieces from their list releases each as a string.
#[test]
fn test_removing_split_pieces_from_their_list_releases_them() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    var parts = "a,bb,ccc,dddd".split(",")
    parts.remove_at(1)
    println(f"{parts.length()} {parts[1]}")
    parts.clear()
    println(f"{parts.length()}")
"#,
        "3 ccc\n0",
    );
}
