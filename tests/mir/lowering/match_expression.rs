// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{mir_snapshot_contains_test, mir_snapshot_test};

#[test]
fn test_match_literal_patterns() {
    // Match generates a switchInt with targets for each literal pattern
    mir_snapshot_test(
        r#"
fn main()
    let x = 2
    match x
        1: "one"
        2: "two"
        _: "other"
"#,
        r#"
            let _0: string;
            let _1: int; // x
            let _2: int;
            let _3: string;
            let _4: string;
            let _5: string;
            let _6: int; // _
            let _7: string;

            bb0: {
                _1 = const Integer(I8(2));
                _2 = _1;
                switchInt(_2) -> [1: bb2, 2: bb3, otherwise: bb4];
            }

            bb1: {
                _0 = _3;
                return;
            }

            bb2: {
                _4 = const String("one");
                goto bb1;
            }

            bb3: {
                _5 = const String("two");
                goto bb1;
            }

            bb4: {
                _6 = _2;
                _7 = const String("other");
                goto bb1;
            }
        "#,
    );
}

#[test]
fn test_match_identifier_binding() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let x = 42
    match x
        n: n
"#,
        &["// x", "// n"],
    );
}

#[test]
fn test_match_multiple_patterns() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let code = 200
    match code
        200 | 201 | 204: "success"
        404: "not found"
        _: "error"
"#,
        &[
            "// code",
            "switchInt",
            r#"String("success")"#,
            r#"String("not found")"#,
        ],
    );
}

#[test]
fn test_match_guard() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let num = 15
    match num
        x if x > 10: "large"
        x: "small"
"#,
        &["// num", "switchInt", "Gt("],
    );
}

#[test]
fn test_nested_match() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let a = 1
    let b = 2
    match a
        1: match b
            2: "inner"
            _: "other inner"
        _: "outer"
"#,
        &[
            "// a",
            "// b",
            "switchInt",
            r#"String("inner")"#,
            r#"String("outer")"#,
        ],
    );
}

#[test]
fn test_match_produces_basic_blocks() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let x = 2
    match x
        1: "one"
        2: "two"
        _: "other"
"#,
        &["bb0:", "bb1:", "bb2:", "bb3:", "bb4:"],
    );
}

#[test]
fn test_match_enum_with_binding() {
    mir_snapshot_contains_test(
        r#"
enum Color: Red(string), Green(string), Blue(string)

fn main()
    let c = Color.Red('#ff0000')
    match c
        Color.Red(x): x
        Color.Green(x): x
        Color.Blue(x): x

"#,
        &["// c", "// x", "switchInt"],
    );
}

#[test]
fn test_match_many_literal_arms() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let x = 5
    match x
        1: "one"
        2: "two"
        3: "three"
        4: "four"
        5: "five"
        6: "six"
        7: "seven"
        _: "other"
"#,
        &["// x", "switchInt", "1: bb", "7: bb"],
    );
}

#[test]
fn test_match_with_expression_in_arm() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let x = 2
    match x
        1: 1 + 1
        2: 2 + 2
        _: 0
"#,
        &["// x", "switchInt", "Add("],
    );
}

#[test]
fn test_match_all_wildcards() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let x = 42
    match x
        _: "any"
"#,
        &["// x", r#"String("any")"#],
    );
}

#[test]
fn test_match_deeply_nested() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let a = 1
    let b = 2
    let c = 3
    match a
        1: match b
            2: match c
                3: "deep"
                _: "not deep c"
            _: "not deep b"
        _: "not deep a"
"#,
        &["// a", "// b", "// c", r#"String("deep")"#],
    );
}

#[test]
fn test_match_result_used() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let x = 1
    let result = match x
        1: 100
        _: 0
"#,
        &["// x", "// result", "const Integer(I8(100))"],
    );
}
