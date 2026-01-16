// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{
    mir_lowering_call_count_test, mir_lowering_min_basic_blocks_test,
    mir_lowering_min_call_count_test,
};

#[test]
fn test_simple_call() {
    let source = "
fn foo() int: 0
fn main()
    let x = foo()
";
    mir_lowering_min_basic_blocks_test(source, 2);
    mir_lowering_min_call_count_test(source, 1);
}

#[test]
fn test_call_with_arguments() {
    let source = "
fn add(a int, b int) int: a + b
fn main()
    let x = add(1, 2)
";
    mir_lowering_min_call_count_test(source, 1);
}

#[test]
fn test_nested_calls() {
    let source = "
fn add(a int, b int) int: a + b
fn mul(a int, b int) int: a * b
fn main()
    let x = add(mul(2, 3), 4)
";
    mir_lowering_call_count_test(source, 2);
}

#[test]
fn test_void_call_statement() {
    let source = "
fn do_something()
    let x = 1
fn main()
    do_something()
";
    mir_lowering_call_count_test(source, 1);
}

#[test]
fn test_call_in_if_condition() {
    let source = "
fn is_ready() bool: true
fn main()
    if is_ready()
        let x = 1
";
    mir_lowering_min_call_count_test(source, 1);
}
