// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_selective_import_runs() {
    let code = r#"
use system.io.{println}
println("selective")
"#;
    assert_runs_with_output(code, "selective");
}

#[test]
fn test_selective_import_rejects_non_imported() {
    // With implicit imports, system.io functions are available globally.
    // This test verifies that selective imports from other modules still
    // properly reject non-selected symbols.
    let code = r#"
use system.string.{String}
let x = is_gpu_available()
"#;
    assert_compiler_error(code, "Undefined variable");
}
