// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `Json.to_string` writes the whole document into one growing text, so its
//! cost in blocks does not scale with the number of values, and a document
//! survives a parse and serialize round trip with no leak or freed-block read.

use super::super::utils::*;

/// A program that builds an array of 1000 strings and then prints `report`.
fn thousand_strings_program(report: &str) -> String {
    format!(
        r#"
use system.json
use system.collections.list

fn main()
    var items = List<Json>()
    var index = 0
    while index < 1000
        items.push(Json.Text("ab"))
        index = index + 1
    {report}
"#
    )
}

#[test]
fn test_json_serializing_a_thousand_strings_allocates_far_fewer_blocks_than_elements() {
    let built = thousand_strings_program(r#"println(f"{items.length()}")"#);
    let written =
        thousand_strings_program(r#"println(f"{Json.Array(items).to_string().length()}")"#);
    let built_blocks = parse_alloc_count_breakdown(&built).rc;
    let written_blocks = parse_alloc_count_breakdown(&written).rc;
    assert!(
        written_blocks < built_blocks + 100,
        "serializing allocates per element: built={built_blocks} written={written_blocks}"
    );
}

/// A document of `count` records mixing nested containers, escapes, control
/// characters, non-ASCII text and every kind of number, written as the body
/// of a Miri string literal.
fn mid_sized_document(count: usize) -> String {
    let records: Vec<String> = (0..count)
        .map(|index| {
            format!(
                r#"{{\"id\":{index},\"name\":\"item\\t{index} \\\"q\\\" caf\\u00e9\",\"tags\":[1,-2.5e3,0.25,true,false,null,[]],\"nested\":{{\"path\":\"a\\\\b\",\"ctl\":\"\\u0001\\n\",\"empty\":{{}}}}}}"#
            )
        })
        .collect();
    format!("[{}]", records.join(","))
}

fn round_trip_program() -> String {
    let source = mid_sized_document(60);
    format!(
        r#"
use system.json
use system.result

fn main()
    match Json.parse("{source}")
        Result.Ok(document)
            let once = document.to_string()
            match Json.parse(once)
                Result.Ok(again)
                    let twice = again.to_string()
                    // Members come out in the map's order, which a reparse
                    // may change, so the two renders are compared by length.
                    println(f"{{once.length() == twice.length()}} {{once.length()}}")
                Result.Err(_): println("reparse failed")
        Result.Err(_): println("parse failed")
"#
    )
}

#[test]
fn test_json_mid_sized_document_round_trips_without_a_leak() {
    assert_runs_with_output(&round_trip_program(), "true 8021");
}

#[test]
fn test_json_mid_sized_document_round_trips_under_the_heap_guard() {
    assert_heap_guard_output(&round_trip_program(), "true 8021");
}

#[test]
fn test_json_writer_is_not_visible_to_an_importing_program() {
    assert_compiler_error(
        r#"
use system.json

fn main()
    let writer = JsonWriter()
    println("reached")
"#,
        "Type 'JsonWriter' is not visible",
    );
}

#[test]
fn test_json_hex_helpers_are_not_visible_to_an_importing_program() {
    assert_compiler_error(
        r#"
use system.json

fn main()
    println(unit_digits(10))
"#,
        "Function 'unit_digits' is not visible",
    );
}
