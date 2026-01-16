// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{
    mir_lowering_basic_blocks_test, mir_lowering_local_test, mir_lowering_locals_test,
    mir_lowering_min_basic_blocks_test, mir_lowering_switch_target_test,
};

#[test]
fn test_simple_if() {
    let source = "
fn main()
    let x = 1
    if true
        let y = 2
    let z = 3
";
    mir_lowering_basic_blocks_test(source, 4);
    mir_lowering_local_test(source, "x");
    mir_lowering_local_test(source, "y");
    mir_lowering_local_test(source, "z");
    mir_lowering_switch_target_test(source, 0, 1);
}

#[test]
fn test_if_else() {
    let source = "
fn main()
    if true
        let a = 1
    else
        let b = 2
    let c = 3
";
    mir_lowering_basic_blocks_test(source, 4);
    mir_lowering_local_test(source, "a");
    mir_lowering_local_test(source, "b");
    mir_lowering_local_test(source, "c");
}

#[test]
fn test_unless() {
    let source = "
fn main()
    unless true
        let a = 1
    else
        let b = 2
";
    mir_lowering_basic_blocks_test(source, 4);
    mir_lowering_switch_target_test(source, 0, 0);
}

#[test]
fn test_nested_if() {
    let source = "
fn main()
    if true
        if false
            let a = 1
        let b = 2
";
    mir_lowering_min_basic_blocks_test(source, 5);
    mir_lowering_local_test(source, "a");
    mir_lowering_local_test(source, "b");
}

#[test]
fn test_deeply_nested_if_else() {
    let source = "
fn main()
    if true
        if true
            if true
                if true
                    let x = 1
";
    mir_lowering_local_test(source, "x");
    mir_lowering_min_basic_blocks_test(source, 9);
}

#[test]
fn test_if_with_complex_condition() {
    let source = "
fn main()
    let a = 5
    let b = 10
    if a < b and b > 0
        let c = 1
";
    mir_lowering_locals_test(source, &["a", "b", "c"]);
}

#[test]
fn test_if_else_chain() {
    let source = "
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
";
    mir_lowering_min_basic_blocks_test(source, 8);
}

#[test]
fn test_unless_with_else() {
    let source = "
fn main()
    unless false
        let a = 1
    else
        let b = 2
    let c = 3
";
    mir_lowering_locals_test(source, &["a", "b", "c"]);
    mir_lowering_basic_blocks_test(source, 4);
}

#[test]
fn test_if_both_branches_return() {
    let source = "
fn main()
    if true
        return
    else
        return
";
    mir_lowering_min_basic_blocks_test(source, 4);
}

#[test]
fn test_if_with_expression_condition() {
    let source = "
fn main()
    let x = 5
    if x + 1 > 5
        let y = 1
";
    mir_lowering_locals_test(source, &["x", "y"]);
}
