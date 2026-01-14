// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::lower_code;
use miri::mir::TerminatorKind;

#[test]
fn test_lower_empty_function() {
    let source = "fn main() int: 0";
    let body = lower_code(source);

    assert_eq!(body.basic_blocks.len(), 1);
    let bb0 = &body.basic_blocks[0];
    assert!(bb0.terminator.is_some());
    if let Some(term) = &bb0.terminator {
        match term.kind {
            TerminatorKind::Return => {}
            _ => panic!("Expected Return terminator"),
        }
    }
}
