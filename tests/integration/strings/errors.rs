// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_string_invalid_method_error() {
    assert_compiler_error(
        r#"

let s = "hello"
let _ = s.nonexistent()
"#,
        "no field or method",
    );
}

#[test]
fn test_a_string_character_cannot_be_assigned_by_index() {
    assert_compiler_error(
        r#"
var s = "abc"
s[0] = "x"
"#,
        "a string's characters are fixed when it is built",
    );
}
