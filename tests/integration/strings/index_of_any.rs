// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

/// A program that prints `found(text, chars, start)` for each call in `calls`,
/// one result per line: the index, or `none`.
fn program(calls: &str) -> String {
    format!(
        r#"
fn found(text String, chars String, start int) String
    match text.index_of_any(chars, start)
        Some(index): f"{{index}}"
        None: "none"

fn main()
{calls}
"#
    )
}

#[test]
fn test_index_of_any_finds_an_ascii_hit_at_the_start_middle_and_end() {
    assert_runs_with_output(
        &program(
            r#"    println(found("abcde", "a", 0))
    println(found("abcde", "xc", 0))
    println(found("abcde", "e", 0))
    println(found("abcde", "zyx", 0))"#,
        ),
        "0\n2\n4\nnone",
    );
}

#[test]
fn test_index_of_any_searches_from_the_start_index() {
    assert_runs_with_output(
        &program(
            r#"    println(found("a,b,c", ",", 0))
    println(found("a,b,c", ",", 2))
    println(found("a,b,c", ",", 4))
    println(found("a,b,c", "c", 4))
    println(found("a,b,c", ",", 5))
    println(found("a,b,c", ",", 99))"#,
        ),
        "1\n3\nnone\n4\nnone\nnone",
    );
}

#[test]
fn test_index_of_any_with_a_negative_start_finds_nothing() {
    assert_runs_with_output(
        &program(
            r#"    println(found("abc", "a", -1))
    println(found("héllo", "l", -3))"#,
        ),
        "none\nnone",
    );
}

#[test]
fn test_index_of_any_with_no_characters_to_find_finds_nothing() {
    assert_runs_with_output(
        &program(
            r#"    println(found("abc", "", 0))
    println(found("", "", 0))
    println(found("", "a", 0))"#,
        ),
        "none\nnone\nnone",
    );
}

#[test]
fn test_index_of_any_counts_characters_past_a_multibyte_character() {
    assert_runs_with_output(
        &program(
            r#"    println(found("héllo, wörld", ",", 0))
    println(found("🦀🦀x🦀y", "y", 0))
    println(found("🦀🦀x🦀y", "xy", 3))
    println(found("aé\"b", "\"", 1))"#,
        ),
        "5\n4\n4\n2",
    );
}

#[test]
fn test_index_of_any_finds_a_multibyte_character_of_the_set() {
    assert_runs_with_output(
        &program(
            r#"    println(found("naïve café", "éï", 0))
    println(found("naïve café", "éï", 3))
    println(found("abc🦀d", "🦀", 0))
    println(found("abcd", "é🦀", 0))"#,
        ),
        "2\n9\n3\nnone",
    );
}

#[test]
fn test_index_of_any_over_text_of_only_multibyte_characters() {
    assert_runs_with_output(
        &program(
            r#"    println(found("日本語のテキスト", "の", 0))
    println(found("日本語のテキスト", "トキ", 5))
    println(found("日本語のテキスト", "a", 0))
    println(found("ééé", "é", 2))
    println(found("ééé", "é", 3))"#,
        ),
        "3\n5\nnone\n2\nnone",
    );
}

#[test]
fn test_index_of_any_refuses_characters_that_are_not_a_string() {
    assert_compiler_error(
        r#"
fn main()
    let found = "abc".index_of_any(5, 0)
"#,
        "Type mismatch for argument 'chars': expected String, got int",
    );
}
