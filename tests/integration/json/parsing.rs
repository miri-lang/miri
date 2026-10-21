// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How much `Json.parse` allocates: a number or a string costs one block for
//! the text it keeps, however many characters it spans.

use super::super::utils::*;

/// A program that parses `json` (written as a Miri string literal body) and
/// prints whether it succeeded, without serializing anything back.
fn parse_program(json: &str) -> String {
    format!(
        r#"
use system.json
use system.result

fn main()
    match Json.parse("{json}")
        Result.Ok(_): println("parsed")
        Result.Err(_): println("rejected")
"#
    )
}

/// The reference-counted blocks a run of `parse_program(json)` allocates.
fn parse_blocks(json: &str) -> usize {
    parse_alloc_count_breakdown(&parse_program(json)).rc
}

#[test]
fn test_json_number_lexeme_is_one_block_however_long() {
    let short = parse_blocks("[7]");
    let long = parse_blocks(&format!(
        "[-{}.{}e+{}]",
        "9".repeat(200),
        "5".repeat(100),
        "3".repeat(50)
    ));
    assert!(
        long < short + 10,
        "a 350-digit number allocates per digit: short={short} long={long}"
    );
}

#[test]
fn test_json_plain_string_costs_no_more_than_a_number() {
    let numbers = ["12"; 200].join(",");
    let strings = [r#"\"ab\""#; 200].join(",");
    let number_blocks = parse_blocks(&format!("[{numbers}]"));
    let string_blocks = parse_blocks(&format!("[{strings}]"));
    assert!(
        string_blocks < number_blocks + 10,
        "an unescaped string allocates more than its text: numbers={number_blocks} strings={string_blocks}"
    );
}

#[test]
fn test_json_invalid_number_lexeme_is_reported_after_valid_digits() {
    assert_runs_with_output(
        r#"
use system.json
use system.result

fn main()
    match Json.parse("[12.5e-3, 4.]")
        Result.Ok(_): println("parsed")
        Result.Err(reason)
            match reason
                JsonError.InvalidNumber(lexeme, line, column): println(f"{lexeme} {line}:{column}")
                default: println("other")
"#,
        "4. 1:11",
    );
}
