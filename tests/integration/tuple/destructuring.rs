// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_tuple_destructuring() {
    assert_runs_with_output(
        r#"
use system.io

let t = (10, 20)
let sum = match t
    (a, b): a + b
println(f"{sum}")
"#,
        "30",
    );
}
