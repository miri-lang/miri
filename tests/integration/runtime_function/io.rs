// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_runtime_io_smoke() {
    assert_runs_with_output(
        r#"
use system.io

runtime fn miri_rt_println(s String)
fn main()
    miri_rt_println("IO Smoke Test")
    println(f"{1}")
"#,
        "1",
    );
}
