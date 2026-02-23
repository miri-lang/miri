// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::assert_runs_with_output;

#[test]
fn test_script_with_functions() {
    let code = r#"
use system.io

fn fib(n int) int
    if n <= 1: n
    else: fib(n - 1) + fib(n - 2)

print(f"{fib(10)}")
"#;
    assert_runs_with_output(code, "55");
}

#[test]
fn test_script_with_main() {
    let code = r#"
use system.io

fn main()
    print("from main")
"#;
    assert_runs_with_output(code, "from main");
}

#[test]
fn test_script_without_main_or_functions() {
    let code = r#"
use system.io
print("just script")
"#;
    assert_runs_with_output(code, "just script");
}
