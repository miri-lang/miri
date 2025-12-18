// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::{check_error, check_success};

#[test]
fn test_generic_implements_constraint_variable() {
    let source = "
struct Interface
    x int

struct Implementation
    x int
    y string

struct Container<T implements Interface>
    val T

let c Container<Implementation>
    ";
    check_success(source);
}

#[test]
fn test_generic_implements_constraint_fail() {
    let source = "
struct Interface
    x int

struct BadImpl
    y string

struct Container<T implements Interface>
    val T

let c Container<BadImpl>
    ";
    check_error(source, "does not satisfy constraint");
}
