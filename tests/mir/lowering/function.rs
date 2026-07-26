// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{mir_lower_code, terminator_of};
use miri::mir::TerminatorKind;

#[test]
fn test_lower_empty_function() {
    let body = mir_lower_code("fn main() int: 0");
    assert_eq!(body.basic_blocks.len(), 1);
    let terminator = terminator_of(&body, 0).expect("Expected a terminator in block 0");
    assert!(matches!(terminator.kind, TerminatorKind::Return));
}
