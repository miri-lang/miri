// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_string_invalid_method_error() {
    assert_compiler_error(
        r#"
use system.string

let s = "hello"
let _ = s.nonexistent()
"#,
        "no field or method",
    );
}
