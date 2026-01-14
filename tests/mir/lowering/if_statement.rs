// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::{has_local, lower_code};
use miri::mir::TerminatorKind;

#[test]
fn test_simple_if() {
    let source = "
fn main()
    let x = 1
    if true
        let y = 2
    let z = 3
";
    let body = lower_code(source);

    // Expected structure:
    // BB0:
    //   x = 1
    //   SwitchInt(true) -> [1: BB1], otherwise: BB2
    // BB1 (then):
    //   y = 2
    //   Goto(BB3)
    // BB2 (else):
    //   Goto(BB3)
    // BB3 (join):
    //   z = 3
    //   Return

    assert_eq!(body.basic_blocks.len(), 4, "Should have 4 basic blocks");

    // Check local presence
    assert!(has_local(&body, "x"));
    assert!(has_local(&body, "y"));
    assert!(has_local(&body, "z"));

    // Check terminators
    let bb0 = &body.basic_blocks[0];
    if let TerminatorKind::SwitchInt { targets, .. } = &bb0.terminator.as_ref().unwrap().kind {
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].0, 1); // 1 = true
    } else {
        panic!("Expected SwitchInt in BB0");
    }
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
    let body = lower_code(source);

    assert_eq!(body.basic_blocks.len(), 4, "Should have 4 basic blocks");
    assert!(has_local(&body, "a"));
    assert!(has_local(&body, "b"));
    assert!(has_local(&body, "c"));
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
    let body = lower_code(source);

    // Structure matches If, but switch targets differ
    assert_eq!(body.basic_blocks.len(), 4);

    let bb0 = &body.basic_blocks[0];
    if let TerminatorKind::SwitchInt { targets, .. } = &bb0.terminator.as_ref().unwrap().kind {
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].0, 0); // 0 = unless condition matches
    } else {
        panic!("Expected SwitchInt in BB0");
    }
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
    let body = lower_code(source);

    // BB0 (outer if)
    // BB1 (outer then) -> BB2 (inner if)
    // BB2 (inner if) -> BB3 (inner then), BB4 (inner else)
    // BB3 -> BB5 (inner join)
    // BB4 -> BB5
    // BB5 -> b=2 -> BB6 (outer join, implicit return?)
    // BB0 else -> BB6...
    // Count will be higher.

    assert!(body.basic_blocks.len() > 4);
    assert!(has_local(&body, "a"));
    assert!(has_local(&body, "b"));
}
