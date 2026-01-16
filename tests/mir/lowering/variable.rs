// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{mir_lowering_assignment_count_test, mir_lowering_local_test};

#[test]
fn test_lower_variable_declaration() {
    mir_lowering_local_test("fn main(): let x = 10", "x");
}

#[test]
fn test_variable_access_and_assignment() {
    let source = "
fn main()
    var x = 1
    var y = x
    x = 2
";
    mir_lowering_local_test(source, "x");
    mir_lowering_local_test(source, "y");
    mir_lowering_assignment_count_test(source, "x", 2);
}
