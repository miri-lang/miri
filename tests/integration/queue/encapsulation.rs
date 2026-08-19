// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_queue_items_field_is_private() {
    assert_compiler_error(
        r#"
use system.collections.queue

fn main()
    let q = Queue<int>()
    q.enqueue(5)
    let x = q.items
"#,
        "Field 'items' of class 'Queue' is Private",
    );
}
