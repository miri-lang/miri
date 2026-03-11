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
    let code = r#"
use system.io.{println}
print("should fail")
"#;
    assert_compiler_error(code, "Undefined variable");
}
