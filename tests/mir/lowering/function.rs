// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{mir_lowering_basic_blocks_test, mir_lowering_return_terminator_test};

#[test]
fn test_lower_empty_function() {
    let source = "fn main() int: 0";
    mir_lowering_basic_blocks_test(source, 1);
    mir_lowering_return_terminator_test(source, 0);
}
