// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::interpreter_assert_runs;

#[test]
fn test_storage_live_dead_check() {
    // Current behavior: StorageLive/Dead are respected and track Uninitialized.
    // We use unindented string to avoid Parser Error.
    interpreter_assert_runs(
        r#"
let x = 10
let y = x + 1
"#,
    );
}

// GPU Launch test moved to src/interpreter/tests.rs because we cannot construct Dim3 from user code yet.
