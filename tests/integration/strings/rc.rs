// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_string_rc_aliasing() {
    assert_runs(
        r#"
use system.io
use system.string

fn consume(s String)
    // s goes out of scope here, should not drop underlying buffer if RC > 1
    let x = 1

fn main()
    let s1 = "hello world"
    let s2 = s1 // IncRef

    consume(s1)
    
    // s2 should still be valid here, no double free or use-after-free
    let len = s2.length()
    
    var s3 = "temporary string"
    s3 = s2 // Reassignment drops "temporary string"
"#,
    );
}
