// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_set_creation() {
    assert_runs("let s = {1, 2, 3}");
}

#[test]
fn test_set_creation_strings() {
    assert_runs("let s = {'a', 'b', 'c'}");
}

#[test]
fn test_set_creation_single() {
    assert_runs("let s = {42}");
}
