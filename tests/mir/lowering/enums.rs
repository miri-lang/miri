// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::{
    lowering_test_aggregate_with_count, lowering_test_has_local, lowering_test_switch_int,
};

// === Basic Enum Variants ===

#[test]
fn test_enum_unit_variant() {
    lowering_test_aggregate_with_count(
        "
enum Status: Ok, Error
fn main()
    let x = Status.Ok
",
        1,
    );
}

#[test]
fn test_enum_single_value_variant() {
    lowering_test_aggregate_with_count(
        "
enum Event: Quit, KeyPress(int)
fn main()
    let x = Event.KeyPress(5)
",
        2,
    );
}

#[test]
fn test_enum_multi_value_variant() {
    lowering_test_aggregate_with_count(
        "
enum Event: Click(int, int)
fn main()
    let x = Event.Click(10, 20)
",
        3,
    );
}

#[test]
fn test_match_enum_variants() {
    lowering_test_switch_int(
        "
enum Status: Ok, Error

fn main()
    let s = Status.Ok
    match s
        Status.Ok: \"ok\"
        Status.Error: \"error\"
",
        1,
    );
}

#[test]
fn test_match_enum_exhaustive() {
    lowering_test_switch_int(
        "
enum Color: Red, Green, Blue
fn main()
    let c = Color.Red
    match c
        Color.Red: 1
        Color.Green: 2
        Color.Blue: 3
",
        1,
    );
}

#[test]
fn test_match_enum_with_binding() {
    lowering_test_has_local(
        "
enum Status: Ok, Error
fn main()
    let s = Status.Ok
    match s
        x: x
",
        "x",
    );
}

#[test]
fn test_enum_with_list_type() {
    lowering_test_aggregate_with_count(
        "
enum Data: Numbers([int])
fn main()
    let d = Data.Numbers([1, 2, 3])
",
        2, // discriminant + list
    );
}

#[test]
fn test_enum_with_map_type() {
    lowering_test_aggregate_with_count(
        r#"
enum Config: Settings({string: int})
fn main()
    let c = Config.Settings({"a": 1})
"#,
        2, // discriminant + map
    );
}

#[test]
fn test_enum_multiple_variants_defined() {
    lowering_test_aggregate_with_count(
        "
enum Event: Quit, KeyPress(int), Click(int, int), Scroll(int)
fn main()
    let x = Event.Scroll(10)
",
        2, // discriminant + value
    );
}
