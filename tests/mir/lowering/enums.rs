// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{
    mir_lowering_local_test, mir_lowering_switch_int_test, mir_lowering_tuple_aggregate_test,
};

#[test]
fn test_enum_unit_variant() {
    mir_lowering_tuple_aggregate_test(
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
    mir_lowering_tuple_aggregate_test(
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
    mir_lowering_tuple_aggregate_test(
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
    mir_lowering_switch_int_test(
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
    mir_lowering_switch_int_test(
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
    mir_lowering_local_test(
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
    mir_lowering_tuple_aggregate_test(
        "
enum Data: Numbers([int])
fn main()
    let d = Data.Numbers([1, 2, 3])
",
        2,
    );
}

#[test]
fn test_enum_with_map_type() {
    mir_lowering_tuple_aggregate_test(
        r#"
enum Config: Settings({string: int})
fn main()
    let c = Config.Settings({"a": 1})
"#,
        2,
    );
}

#[test]
fn test_enum_multiple_variants_defined() {
    mir_lowering_tuple_aggregate_test(
        "
enum Event: Quit, KeyPress(int), Click(int, int), Scroll(int)
fn main()
    let x = Event.Scroll(10)
",
        2,
    );
}
