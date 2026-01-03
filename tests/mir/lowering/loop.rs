// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::super::utils::{has_local, lower_code};
use miri::mir::TerminatorKind;

#[test]
fn test_while_loop() {
    let source = "
fn main()
    var x = 0
    while x < 10
        x = x + 1
    let y = 1
";
    let body = lower_code(source);
    assert!(has_local(&body, "x"));
    assert!(has_local(&body, "y"));

    // Structure:
    // BB0: x = 0, Goto BB1
    // BB1 (Header): SwitchInt(x<10) -> [1: BB2], otherwise BB3
    // BB2 (Body): x = x + 1, Goto BB1
    // BB3 (Exit): y = 1, Return

    assert!(body.basic_blocks.len() >= 4);

    // Check Header Terminator
    let header_bb = &body.basic_blocks[1];
    if let TerminatorKind::SwitchInt { .. } = header_bb.terminator.as_ref().unwrap().kind {
        // Ok
    } else {
        panic!("Expected SwitchInt in header");
    }
}

#[test]
fn test_until_loop() {
    let source = "
fn main()
    var x = 0
    until x == 10
        x = x + 1
";
    let body = lower_code(source);

    // Should use SwitchInt with 0 -> Body for Until
    let header_bb = &body.basic_blocks[1];
    if let TerminatorKind::SwitchInt { targets, .. } = &header_bb.terminator.as_ref().unwrap().kind
    {
        assert_eq!(targets[0].0, 0); // 0 = false (until condition false -> loop continues)
    } else {
        panic!("Expected SwitchInt in header");
    }
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
    let body = lower_code(source);

    // Structure:
    // BB0: x=0, Goto BB1 (Body)
    // BB1 (Body): x=x+1, Goto BB2 (Cond)
    // BB2 (Cond): SwitchInt(x<10) -> [1: BB1], otherwise BB3 (Exit)

    assert!(body.basic_blocks.len() >= 4);
}

#[test]
fn test_forever_loop_break() {
    let source = "
fn main()
    forever
        break
";
    let body = lower_code(source);

    // BB0: Goto BB1 (Body)
    // BB1 (Body): Goto BB2 (Break target)
    // BB2 (Exit): Return

    assert_eq!(body.basic_blocks.len(), 3);

    let body_bb = &body.basic_blocks[1];
    if let TerminatorKind::Goto { target } = body_bb.terminator.as_ref().unwrap().kind {
        assert_eq!(target.0, 2);
    } else {
        panic!("Expected Goto exit");
    }
}

#[test]
fn test_for_loop() {
    let source = "
fn main()
    for i in 0..10
        let x = i
";
    let body = lower_code(source);

    // BB0: Init i, Goto BB1
    // BB1 (Header): Cond, SwitchInt -> BB2, else BB4
    // BB2 (Body): x=i, Goto BB3
    // BB3 (Inc): i=i+1, Goto BB1
    // BB4 (Exit)

    assert!(has_local(&body, "i"));
    assert!(has_local(&body, "x"));

    assert!(body.basic_blocks.len() >= 5);
}

#[test]
fn test_continue_in_while() {
    let source = "
fn main()
    while true
        continue
";
    let body = lower_code(source);

    // BB2 (Body) should Goto BB1 (Header) due to continue
    let body_bb = &body.basic_blocks[2];
    if let TerminatorKind::Goto { target } = body_bb.terminator.as_ref().unwrap().kind {
        assert_eq!(target.0, 1);
    } else {
        panic!("Expected Goto header");
    }
}

#[test]
fn test_continue_in_for() {
    let source = "
fn main()
    for i in 0..10
        continue
";
    let body = lower_code(source);

    // BB2 (Body) should Goto BB3 (Increment) due to continue
    let body_bb = &body.basic_blocks[2];
    if let TerminatorKind::Goto { target } = body_bb.terminator.as_ref().unwrap().kind {
        // BB3 is increment
        assert_eq!(target.0, 3);
    } else {
        panic!("Expected Goto increment");
    }
}

#[test]
#[should_panic]
fn test_break_outside_loop() {
    let source = "
fn main()
    break
";
    lower_code(source);
}
