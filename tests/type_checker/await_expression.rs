// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{type_checker_error_test, type_checker_expr_type_test, type_checker_test};
use miri::ast::factory::*;

#[test]
fn test_await_future_variable() {
    type_checker_expr_type_test(
        "
let f Future<int>
await f
",
        type_int(),
    );
}

#[test]
fn test_await_nested_future() {
    type_checker_expr_type_test(
        "
let f Future<Future<String>>
await await f
",
        type_string(),
    );
}

#[test]
fn test_await_non_future_error() {
    type_checker_error_test(
        "
let x = 10
await x
",
        "Await requires a Future",
    );
}

#[test]
fn test_await_in_expression() {
    type_checker_expr_type_test(
        "
let f Future<int>
(await f) + 1
",
        type_int(),
    );
}

#[test]
fn test_await_function_call() {
    type_checker_expr_type_test(
        "
fn get_future() Future<int>
    let f Future<int>
    return f

await get_future()
",
        type_int(),
    );
}

#[test]
fn test_await_list_future() {
    type_checker_expr_type_test(
        "
let f Future<List<int>>
await f
",
        type_list(type_int()),
    );
}

#[test]
fn test_await_map_future() {
    type_checker_expr_type_test(
        "
let f Future<Map<String, int>>
await f
",
        type_map(type_string(), type_int()),
    );
}

#[test]
fn test_await_void_future() {
    type_checker_expr_type_test(
        "
let f Future<void>
await f
",
        type_custom("void", None),
    );
}

#[test]
fn test_await_nullable_future() {
    type_checker_expr_type_test(
        "
let f Future<int?>
await f
",
        type_null(type_int()),
    );
}

#[test]
fn test_await_custom_type() {
    type_checker_expr_type_test(
        "
struct User
    name String

let f Future<User>
await f
",
        type_custom("User", None),
    );
}

#[test]
fn test_await_in_async_function() {
    // Await inside an async function is allowed
    type_checker_expr_type_test(
        "
async fn process(f Future<int>) int
    await f

let x Future<int>
process(x)
",
        type_int(),
    );
}

#[test]
fn test_await_in_non_async_function_error() {
    // Await inside a non-async function is not allowed
    type_checker_error_test(
        "
fn process(f Future<int>) int
    await f

let x Future<int>
process(x)
",
        "'await' can only be used in async functions or at the top level",
    );
}

#[test]
fn test_await_in_conditional_expression() {
    type_checker_expr_type_test(
        "
let f Future<bool>
1 if await f else 0
",
        type_int(),
    );
}

#[test]
fn test_await_in_loop_condition() {
    type_checker_test(
        "
let f Future<bool>
while await f
    break
",
    );
}
