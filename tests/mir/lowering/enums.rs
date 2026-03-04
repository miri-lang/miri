// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{mir_snapshot_contains_test, mir_snapshot_test};

#[test]
fn test_enum_unit_variant() {
    // Unit variant is represented as Enum aggregate with discriminant
    // (1-indexed due to BTreeMap ordering: Error=0, Ok=1)
    mir_snapshot_test(
        r#"
enum Status: Ok, Error
fn main()
    let x = Status.Ok
"#,
        r#"
            let _0: void;
            let _1: Status; // x

            bb0: {
                StorageLive(_1);
                _1 = Status.Ok(const Integer(I32(1)));
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_enum_single_value_variant() {
    mir_snapshot_contains_test(
        r#"
enum Event: Quit, KeyPress(int)
fn main()
    let x = Event.KeyPress(5)
"#,
        &[
            "// x",
            "Event.KeyPress(const Integer(I32(0)), const Integer(I8(5)))",
        ],
    );
}

#[test]
fn test_enum_multi_value_variant() {
    mir_snapshot_contains_test(
        r#"
enum Event: Click(int, int)
fn main()
    let x = Event.Click(10, 20)
"#,
        &[
            "// x",
            "Event.Click(",
            "const Integer(I8(10))",
            "const Integer(I8(20))",
        ],
    );
}

#[test]
fn test_match_enum_variants() {
    mir_snapshot_contains_test(
        r#"
enum Status: Ok, Error

fn main()
    let s = Status.Ok
    match s
        Status.Ok: "ok"
        Status.Error: "error"
"#,
        &["// s", "switchInt", r#"String("ok")"#, r#"String("error")"#],
    );
}

#[test]
fn test_match_enum_exhaustive() {
    mir_snapshot_contains_test(
        r#"
enum Color: Red, Green, Blue
fn main()
    let c = Color.Red
    match c
        Color.Red: 1
        Color.Green: 2
        Color.Blue: 3
"#,
        &["// c", "switchInt", "bb2:", "bb3:", "bb4:"],
    );
}

#[test]
fn test_match_enum_with_binding() {
    mir_snapshot_contains_test(
        r#"
enum Status: Ok, Error
fn main()
    let s = Status.Ok
    match s
        x: x
"#,
        &["// s", "// x"],
    );
}

#[test]
fn test_enum_with_array_type() {
    mir_snapshot_contains_test(
        r#"
enum Data: Numbers([int; 3])
fn main()
    let d = Data.Numbers([1, 2, 3])
"#,
        &["// d", "Data", "[const Integer"],
    );
}

#[test]
fn test_enum_multiple_variants_defined() {
    mir_snapshot_contains_test(
        r#"
enum Event: Quit, KeyPress(int), Click(int, int), Scroll(int)
fn main()
    let x = Event.Scroll(10)
"#,
        &["// x", "Event", "const Integer(I8(10))"],
    );
}
