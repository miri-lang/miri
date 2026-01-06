// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::{lowering_test_has_local, lowering_test_switch_int};
use crate::mir::utils::lower_code;

#[test]
fn test_match_literal_patterns() {
    lowering_test_switch_int(
        "
fn main()
    let x = 2
    match x
        1: \"one\"
        2: \"two\"
        _: \"other\"
",
        1, // At least 1 SwitchInt for the match
    );
}

#[test]
fn test_match_identifier_binding() {
    lowering_test_has_local(
        "
fn main()
    let x = 42
    match x
        n: n
",
        "n",
    );
}

#[test]
fn test_match_multiple_patterns() {
    lowering_test_switch_int(
        "
fn main()
    let code = 200
    match code
        200 | 201 | 204: \"success\"
        404: \"not found\"
        _: \"error\"
",
        1,
    );
}

#[test]
fn test_match_guard() {
    // Guards produce additional SwitchInt for the condition
    lowering_test_switch_int(
        "
fn main()
    let num = 15
    match num
        x if x > 10: \"large\"
        x: \"small\"
",
        2, // Main switch + guard condition
    );
}

#[test]
fn test_nested_match() {
    // Nested match produces multiple SwitchInt terminators
    lowering_test_switch_int(
        "
fn main()
    let a = 1
    let b = 2
    match a
        1: match b
            2: \"inner\"
            _: \"other inner\"
        _: \"outer\"
",
        2, // Outer + inner match
    );
}

#[test]
fn test_match_produces_basic_blocks() {
    let body = lower_code(
        "
fn main()
    let x = 2
    match x
        1: \"one\"
        2: \"two\"
        _: \"other\"
",
    );
    // At least: entry, 3 branches, join = 5 blocks
    assert!(
        body.basic_blocks.len() >= 5,
        "Expected at least 5 basic blocks"
    );
}

#[test]
fn test_match_enum_with_binding() {
    lowering_test_has_local(
        "
enum Color: Red(string), Green(string), Blue(string)

fn main()
    let c = Color.Red('#ff0000')
    match c
        Color.Red(x): x
        Color.Green(x): x
        Color.Blue(x): x

",
        "x",
    );
}
