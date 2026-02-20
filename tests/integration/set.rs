// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::assert_runs;

#[test]
fn test_set_creation() {
    assert_runs("let s = {1, 2, 3}");
}
