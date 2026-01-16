// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_lowering_pretty_print_contains_test;

#[test]
fn test_mir_pretty_print_function_body() {
    let source = "
fn add(a int, b int) int
    let result = a + b
    result
";
    mir_lowering_pretty_print_contains_test(source, "let _");
    mir_lowering_pretty_print_contains_test(source, "int");
    mir_lowering_pretty_print_contains_test(source, "bb0:");
    mir_lowering_pretty_print_contains_test(source, "return");
}

#[test]
fn test_mir_pretty_print_binary_op() {
    let source = "fn main(): 1 + 2";
    mir_lowering_pretty_print_contains_test(source, "Add");
}

#[test]
fn test_mir_pretty_print_local_names() {
    let source = "
fn main()
    let x = 10
    let y = 20
";
    mir_lowering_pretty_print_contains_test(source, "// x");
    mir_lowering_pretty_print_contains_test(source, "// y");
}

#[test]
fn test_mir_pretty_print_blocks() {
    let source = "
fn main()
    if true
        let a = 1
    else
        let b = 2
";
    mir_lowering_pretty_print_contains_test(source, "bb0:");
    mir_lowering_pretty_print_contains_test(source, "bb1:");
    mir_lowering_pretty_print_contains_test(source, "switchInt");
}

#[test]
fn test_mir_pretty_print_return_terminator() {
    let source = "fn main(): 42";
    mir_lowering_pretty_print_contains_test(source, "return;");
}
