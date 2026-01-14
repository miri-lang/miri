// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::lower_code;
use miri::mir::TerminatorKind;

#[test]
fn test_simple_call() {
    let source = "
fn foo() int: 0
fn main()
    let x = foo()
";
    // We expect a call terminator in the block where x is initialized.
    let body = lower_code(source);

    // There should be at least two blocks:
    // 0: call foo() -> 1
    // 1: x = return_val

    assert!(
        body.basic_blocks.len() >= 2,
        "Should have multiple blocks due to call"
    );

    let first_block = &body.basic_blocks[0];
    if let Some(terminator) = &first_block.terminator {
        if let TerminatorKind::Call {
            func: _func,
            args: _args,
            destination: _destination,
            target,
        } = &terminator.kind
        {
            assert!(target.is_some(), "Call should have a target block");
            // Check implicit return type handling if possible
        } else {
            panic!("Expected Call terminator, got {:?}", terminator.kind);
        }
    } else {
        panic!("First block should have a terminator");
    }
}

#[test]
fn test_call_with_arguments() {
    let source = "
fn add(a int, b int) int: a + b
fn main()
    let x = add(1, 2)
";
    let body = lower_code(source);

    let first_block = &body.basic_blocks[0];
    if let Some(terminator) = &first_block.terminator {
        if let TerminatorKind::Call {
            func: _func, args, ..
        } = &terminator.kind
        {
            assert_eq!(args.len(), 2, "Should have 2 arguments");
        } else {
            panic!("Expected Call terminator");
        }
    }
}

#[test]
fn test_nested_calls() {
    let source = "
fn add(a int, b int) int: a + b
fn mul(a int, b int) int: a * b
fn main()
    let x = add(mul(2, 3), 4)
";
    // Execution order:
    // 1. mul(2, 3) -> temp1
    // 2. add(temp1, 4) -> x

    // This implies multiple basic blocks chained by calls.
    let body = lower_code(source);

    // Verify we have multiple call terminators in the sequence
    let calls_count = body
        .basic_blocks
        .iter()
        .filter(|bb| {
            if let Some(term) = &bb.terminator {
                matches!(term.kind, TerminatorKind::Call { .. })
            } else {
                false
            }
        })
        .count();

    assert_eq!(calls_count, 2, "Should have 2 calls");
}

#[test]
fn test_void_call_statement() {
    let source = "
fn do_something()
    let x = 1
fn main()
    do_something()
";
    // Should emit a call, and the result is discarded (or assigned to temp and discarded)
    let body = lower_code(source);

    let calls_count = body
        .basic_blocks
        .iter()
        .filter(|bb| {
            if let Some(term) = &bb.terminator {
                matches!(term.kind, TerminatorKind::Call { .. })
            } else {
                false
            }
        })
        .count();

    assert_eq!(calls_count, 1, "Should have 1 call");
}

#[test]
fn test_call_in_if_condition() {
    let source = "
fn is_ready() bool: true
fn main()
    if is_ready()
        let x = 1
";
    // condition evaluation involves a call.
    // block 0: call is_ready() -> block 1
    // block 1: switchInt(result) -> ...

    let body = lower_code(source);

    let calls_count = body
        .basic_blocks
        .iter()
        .filter(|bb| {
            if let Some(term) = &bb.terminator {
                matches!(term.kind, TerminatorKind::Call { .. })
            } else {
                false
            }
        })
        .count();

    assert!(calls_count >= 1, "Should have call in condition");
}
