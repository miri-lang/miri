// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

/// Regression test: EnumValue (Option::Some syntax) must support DPS.
/// Previously, EnumValue always allocated a fresh temp and ignored the
/// caller-provided destination, leaving the destination uninitialized.
#[test]
fn test_enum_value_dps_in_match_result() {
    assert_runs_with_output(
        r#"
use system.io

enum MyOption: Some(int), None

fn make_option(x int) MyOption
    MyOption.Some(x)

fn main()
    let opt = make_option(42)
    let result = match opt
        MyOption.None: 0
        MyOption.Some(v): v
    print(f"{result}")
"#,
        "42",
    );
}

/// Regression test: enum variant constructor via Call path must work when used
/// as a variable initializer (DPS passes dest to the lowering).
#[test]
fn test_enum_variant_constructor_call_dps() {
    assert_runs_with_output(
        r#"
use system.io

enum Result: Ok(int), Err(int)

fn main()
    let r = Result.Ok(100)
    let val = match r
        Result.Ok(v): v
        Result.Err(e): e
    print(f"{val}")
"#,
        "100",
    );
}
