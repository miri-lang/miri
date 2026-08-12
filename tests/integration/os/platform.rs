// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_run;

#[test]
fn test_platform_returns_string() {
    let code = r#"
use system.result
use system.os

fn main()
    let p = platform()
    println(p)
"#;
    let result = miri_run(code);
    assert!(
        result.success,
        "Platform retrieval failed:\n{}",
        result.output()
    );

    let expected = std::env::consts::OS;
    assert!(
        result.output().contains(expected),
        "Expected platform name '{}' in output:\n{}",
        expected,
        result.output()
    );
}

#[test]
fn test_platform_is_exactly_the_host_name() {
    let code = r#"
use system.result
use system.os

fn main()
    println(platform())
"#;
    let result = miri_run(code);
    assert!(
        result.success,
        "Platform retrieval failed:\n{}",
        result.output()
    );
    assert_eq!(
        result.stdout.trim(),
        std::env::consts::OS,
        "platform() must report the host name exactly, got:\n{}",
        result.output()
    );
}
