// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn map_length() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

let m = {"a": 1, "b": 2, "c": 3}
println(f"{m.length()}")
"#,
        "3",
    );
}
