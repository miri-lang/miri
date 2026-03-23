// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_local_module_not_found() {
    assert_project_compiler_error(
        &[("main.mi", "use local.missing.helper\nlet x = 1\n")],
        "Module 'local.missing.helper' not found",
    );
}
