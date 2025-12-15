// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use miri::ast::Type;
use super::utils::{check_expr_type, check_error};

#[test]
fn test_await_future_variable() {
    check_expr_type("
let f future<int>
await f
", Type::Int);
}

#[test]
fn test_await_nested_future() {
    check_expr_type("
let f future<future<string>>
await await f
", Type::String);
}

#[test]
fn test_await_non_future_error() {
    check_error("
let x = 10
await x
", "Await requires a Future");
}

#[test]
fn test_await_in_expression() {
    check_expr_type("
let f future<int>
(await f) + 1
", Type::Int);
}

