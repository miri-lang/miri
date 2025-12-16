// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_match_expression_basic() {
    let source = "
    let x = 1
    let res = match x
        1: 'one'
        2: 'two'
        default: 'other'
    ";
    check_success(source);
}

#[test]
fn test_match_expression_type_mismatch() {
    let source = "
    let x = 1
    let res = match x
        1: 'one'
        2: 2
    ";
    check_error(source, "Match branch types mismatch");
}

#[test]
fn test_match_pattern_variable_binding() {
    let source = "
    let x = 10
    let res = match x
        val if val > 5: 'large'
        default: 'small'
    ";
    check_success(source);
}

#[test]
fn test_match_pattern_type_mismatch() {
    let source = "
    let x = 1
    match x
        'one': 'one'
    ";
    check_error(source, "Pattern type mismatch");
}
