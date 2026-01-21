// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{mir_snapshot_contains_test, mir_snapshot_test};

#[test]
fn test_simple_if() {
    // Demonstrates the basic if-statement control flow:
    // bb0: entry block with condition check
    // bb1: then branch
    // bb2: empty else branch (still generated)
    // bb3: join block after if
    mir_snapshot_test(
        r#"
fn main()
    let x = 1
    if true
        let y = 2
    let z = 3
"#,
        r#"
            let _0: void;
            let _1: int; // x
            let _2: int; // y
            let _3: int; // z

            bb0: {
                StorageLive(_1);
                _1 = const Integer(I8(1));
                switchInt(const Boolean(true)) -> [1: bb1, otherwise: bb2];
            }

            bb1: {
                StorageLive(_2);
                _2 = const Integer(I8(2));
                StorageDead(_2);
                goto bb3;
            }

            bb2: {
                goto bb3;
            }

            bb3: {
                StorageLive(_3);
                _3 = const Integer(I8(3));
                StorageDead(_3);
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_if_else() {
    mir_snapshot_contains_test(
        r#"
fn main()
    if true
        let a = 1
    else
        let b = 2
    let c = 3
"#,
        &[
            "// a",
            "// b",
            "// c",
            "switchInt",
            "goto bb3", // both branches join at bb3
        ],
    );
}

#[test]
fn test_unless() {
    // unless is like if with inverted condition logic
    // [0: bb1] means when false go to bb1 (the then-branch for unless)
    mir_snapshot_contains_test(
        r#"
fn main()
    unless true
        let a = 1
    else
        let b = 2
"#,
        &["// a", "// b", "switchInt", "[0: bb1, otherwise: bb2]"],
    );
}

#[test]
fn test_nested_if() {
    mir_snapshot_contains_test(
        r#"
fn main()
    if true
        if false
            let a = 1
        let b = 2
"#,
        &[
            "// a", "// b", "bb0:", "bb1:", "bb2:", "bb3:", "bb4:", // at least 5 basic blocks
        ],
    );
}

#[test]
fn test_deeply_nested_if_else() {
    mir_snapshot_contains_test(
        r#"
fn main()
    if true
        if true
            if true
                if true
                    let x = 1
"#,
        &[
            "// x", "bb8:", // deep nesting creates many blocks
        ],
    );
}

#[test]
fn test_if_with_complex_condition() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let a = 5
    let b = 10
    if a < b and b > 0
        let c = 1
"#,
        &["// a", "// b", "// c", "Lt(", "Gt("],
    );
}

#[test]
fn test_if_else_chain() {
    mir_snapshot_contains_test(
        r#"
fn main()
    let x = 5
    if x == 1
        let a = 1
    else
        if x == 2
            let b = 2
        else
            if x == 3
                let c = 3
            else
                let d = 4
"#,
        &[
            "// a", "// b", "// c", "// d", "bb7:", // chained if-else creates many blocks
        ],
    );
}

#[test]
fn test_if_both_branches_return() {
    mir_snapshot_contains_test(
        r#"
fn main()
    if true
        return
    else
        return
"#,
        &["return;", "bb1:", "bb2:", "switchInt"],
    );
}
