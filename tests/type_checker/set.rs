// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_set_literal() {
    check_success("{ 1, 2, 3 }");
}

#[test]
fn test_set_literal_mixed_error() {
    check_error("{ 1, \"a\" }", "Set elements must have the same type");
}
