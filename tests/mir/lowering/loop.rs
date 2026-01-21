// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{mir_lower_code, mir_snapshot_contains_test, mir_snapshot_test};

#[test]
fn test_while_loop() {
    // Demonstrates  while loop control flow:
    // bb0: initialization
    // bb1: loop header (condition check)
    // bb2: loop body
    // bb3: exit block
    mir_snapshot_test(
        r#"
fn main()
    var x = 0
    while x < 10
        x = x + 1
    let y = 1
"#,
        r#"
            let _0: void;
            let _1: int; // x
            let _2: boolean;
            let _3: int;
            let _4: int;
            let _5: int; // y

            bb0: {
                StorageLive(_1);
                _1 = const Integer(I8(0));
                goto bb1;
            }

            bb1: {
                _2 = Lt(_1, const Integer(I8(10)));
                switchInt(_2) -> [1: bb2, otherwise: bb3];
            }

            bb2: {
                _3 = Add(_1, const Integer(I8(1)));
                _1 = _3;
                _4 = _3;
                goto bb1;
            }

            bb3: {
                StorageLive(_5);
                _5 = const Integer(I8(1));
                StorageDead(_5);
                StorageDead(_1);
                return;
            }
        "#,
    );
}

#[test]
fn test_until_loop() {
    // until loop: condition inverted (exits when condition becomes true)
    mir_snapshot_contains_test(
        r#"
fn main()
    var x = 0
    until x == 10
        x = x + 1
"#,
        &["// x", "Eq(", "switchInt(_", "[0: bb2, otherwise: bb3]"],
    );
}

#[test]
fn test_do_while_loop() {
    // do-while executes body at least once before checking condition
    mir_snapshot_contains_test(
        r#"
fn main()
    var x = 0
    do
        x = x + 1
    while x < 10
"#,
        &["// x", "Lt(", "goto bb1", "switchInt"],
    );
}

#[test]
fn test_forever_loop_break() {
    // forever loop with break generates unconditional loop with break target
    mir_snapshot_contains_test(
        r#"
fn main()
    forever
        break
"#,
        &["goto bb1", "goto bb2"],
    );
}

#[test]
fn test_for_loop() {
    mir_snapshot_contains_test(
        r#"
fn main()
    for i in 0..10
        let x = i
"#,
        &["// i", "// x", "Lt(", "Add(", "goto bb1"],
    );
}

#[test]
fn test_continue_in_while() {
    mir_snapshot_contains_test(
        r#"
fn main()
    while true
        continue
"#,
        &["goto bb1"], // continue jumps back to loop header
    );
}

#[test]
fn test_continue_in_for() {
    mir_snapshot_contains_test(
        r#"
fn main()
    for i in 0..10
        continue
"#,
        &["// i", "goto bb3"], // continue jumps to increment block
    );
}

#[test]
#[should_panic]
fn test_break_outside_loop() {
    let source = r#"
fn main()
    break
"#;
    mir_lower_code(source);
}

#[test]
#[should_panic]
fn test_continue_outside_loop() {
    let source = r#"
fn main()
    continue
"#;
    mir_lower_code(source);
}

#[test]
fn test_nested_while_loops() {
    mir_snapshot_contains_test(
        r#"
fn main()
    var i = 0
    while i < 10
        var j = 0
        while j < 10
            j = j + 1
        i = i + 1
"#,
        &["// i", "// j", "bb5:", "bb1:"], // nested loops create more blocks
    );
}

#[test]
fn test_deeply_nested_loops() {
    mir_snapshot_contains_test(
        r#"
fn main()
    for a in 0..2
        for b in 0..2
            for c in 0..2
                let x = a + b + c
"#,
        &["// a", "// b", "// c", "// x"],
    );
}

#[test]
fn test_for_loop_descending() {
    mir_snapshot_contains_test(
        r#"
fn main()
    for i in 10..0
        let x = i
"#,
        &["// i", "// x"],
    );
}

#[test]
fn test_while_with_complex_condition() {
    mir_snapshot_contains_test(
        r#"
fn main()
    var x = 0
    while x < 10 and x >= 0
        x = x + 1
"#,
        &["// x", "Lt(", "Ge("],
    );
}

#[test]
fn test_loop_with_early_break() {
    mir_snapshot_contains_test(
        r#"
fn main()
    forever
        let x = 1
        break
"#,
        &["// x", "goto bb2"], // break exits to bb2
    );
}

#[test]
fn test_multiple_breaks_in_different_branches() {
    mir_snapshot_contains_test(
        r#"
fn main()
    forever
        if true
            break
        else
            break
"#,
        &["switchInt", "goto bb"], // both branches break out
    );
}
