// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_runtime_alloc_free() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_alloc(size int, align int) i64
runtime fn miri_free(ptr i64, size int, align int)

fn main()
    let ptr = miri_alloc(1024, 8)
    let result = if ptr == 0
        0
    else
        miri_free(ptr, 1024, 8)
        1
    println(f"{result}")
"#,
        "1",
    );
}

#[test]
fn test_runtime_realloc() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_alloc(size int, align int) i64
runtime fn miri_realloc(ptr i64, old_size int, align int, new_size int) i64
runtime fn miri_free(ptr i64, size int, align int)

fn main()
    let ptr = miri_alloc(64, 8)
    let new_ptr = miri_realloc(ptr, 64, 8, 128)

    let result = if new_ptr == 0
        0
    else
        miri_free(new_ptr, 128, 8)
        1
    println(f"{result}")
"#,
        "1",
    );
}
