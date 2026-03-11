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
