// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_float_types() {
    assert_runs("let x f32 = 3.14");
    assert_runs("let x f64 = 3.14159265358979");
}

#[test]
fn test_float_operations() {
    assert_runs("1.5 + 2.5");
    assert_runs("3.0 * 2.0");
    assert_runs("10.0 / 4.0");
}

#[test]
fn test_f32_formatting_uses_f32_precision() {
    // An `f32` must format using its own shortest round-trip representation, not
    // be promoted to `f64` first — promotion exposes the f32→f64 representation
    // error (`0.1f32` becomes `0.10000000149011612`). The bracket sentinels pin
    // the whole rendering, since the output assertion is a substring match.
    assert_runs_with_output(
        r#"
let x f32 = 0.1
println(f"[{x}]")
"#,
        "[0.1]",
    );
    assert_runs_with_output(
        r#"
let x f32 = 3.14
println(f"[{x}]")
"#,
        "[3.14]",
    );
    // A whole-number f32 keeps the one-decimal-place convention used for floats.
    assert_runs_with_output(
        r#"
let x f32 = 3.0
println(f"[{x}]")
"#,
        "[3.0]",
    );
}
