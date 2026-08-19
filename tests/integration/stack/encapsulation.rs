// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_stack_items_field_is_private() {
    assert_compiler_error(
        r#"
use system.collections.stack

fn main()
    let s = Stack<int>()
    s.push(5)
    let x = s.items
"#,
        "Field 'items' of class 'Stack' is Private",
    );
}
