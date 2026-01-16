// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{
    mir_lower_code, mir_lowering_basic_blocks_test, mir_lowering_goto_target_test,
    mir_lowering_local_test, mir_lowering_min_basic_blocks_test, mir_lowering_switch_target_test,
};

#[test]
fn test_while_loop() {
    let source = "
fn main()
    var x = 0
    while x < 10
        x = x + 1
    let y = 1
";
    mir_lowering_local_test(source, "x");
    mir_lowering_local_test(source, "y");
    mir_lowering_min_basic_blocks_test(source, 4);
}

#[test]
fn test_until_loop() {
    let source = "
fn main()
    var x = 0
    until x == 10
        x = x + 1
";
    mir_lowering_switch_target_test(source, 1, 0);
}

#[test]
fn test_do_while_loop() {
    let source = "
fn main()
    var x = 0
    do
        x = x + 1
    while x < 10
";
    mir_lowering_min_basic_blocks_test(source, 4);
}

#[test]
fn test_forever_loop_break() {
    let source = "
fn main()
    forever
        break
";
    mir_lowering_basic_blocks_test(source, 3);
    mir_lowering_goto_target_test(source, 1, 2);
}

#[test]
fn test_for_loop() {
    let source = "
fn main()
    for i in 0..10
        let x = i
";
    mir_lowering_local_test(source, "i");
    mir_lowering_local_test(source, "x");
    mir_lowering_min_basic_blocks_test(source, 5);
}

#[test]
fn test_continue_in_while() {
    let source = "
fn main()
    while true
        continue
";
    mir_lowering_goto_target_test(source, 2, 1);
}

#[test]
fn test_continue_in_for() {
    let source = "
fn main()
    for i in 0..10
        continue
";
    mir_lowering_goto_target_test(source, 2, 3);
}

#[test]
#[should_panic]
fn test_break_outside_loop() {
    let source = "
fn main()
    break
";
    mir_lower_code(source);
}

#[test]
#[should_panic]
fn test_continue_outside_loop() {
    let source = "
fn main()
    continue
";
    mir_lower_code(source);
}

#[test]
fn test_nested_while_loops() {
    let source = "
fn main()
    var i = 0
    while i < 10
        var j = 0
        while j < 10
            j = j + 1
        i = i + 1
";
    mir_lowering_local_test(source, "i");
    mir_lowering_local_test(source, "j");
    mir_lowering_min_basic_blocks_test(source, 6);
}

#[test]
fn test_deeply_nested_loops() {
    let source = "
fn main()
    for a in 0..2
        for b in 0..2
            for c in 0..2
                let x = a + b + c
";
    mir_lowering_local_test(source, "a");
    mir_lowering_local_test(source, "b");
    mir_lowering_local_test(source, "c");
    mir_lowering_local_test(source, "x");
}

#[test]
fn test_for_loop_descending() {
    let source = "
fn main()
    for i in 10..0
        let x = i
";
    mir_lowering_local_test(source, "i");
}

#[test]
fn test_while_with_complex_condition() {
    let source = "
fn main()
    var x = 0
    while x < 10 and x >= 0
        x = x + 1
";
    mir_lowering_local_test(source, "x");
    mir_lowering_min_basic_blocks_test(source, 4);
}

#[test]
fn test_loop_with_early_break() {
    let source = "
fn main()
    forever
        let x = 1
        break
";
    mir_lowering_local_test(source, "x");
    mir_lowering_basic_blocks_test(source, 3);
}

#[test]
fn test_multiple_breaks_in_different_branches() {
    let source = "
fn main()
    forever
        if true
            break
        else
            break
";
    mir_lowering_min_basic_blocks_test(source, 5);
}
