// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use crate::type_checker::utils::check_errors;

#[test]
fn test_multiple_errors() {
    let source = "
    let x int = \"string\"
    let y bool = 123
    ";
    
    check_errors(source, vec![
        "Type mismatch for variable 'x': expected Int, got String",
        "Type mismatch for variable 'y': expected Boolean, got Int"
    ]);
}
