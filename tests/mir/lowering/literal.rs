// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_lowering_literal_i8_test;

#[test]
fn test_lower_literal() {
    mir_lowering_literal_i8_test("fn main(): 42", 42);
}
