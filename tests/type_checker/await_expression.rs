// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::{check_error, check_expr_type, check_success};
use miri::ast::factory::*;

#[test]
fn test_await_future_variable() {
    check_expr_type(
        "
let f future<int>
await f
",
        type_int(),
    );
}

#[test]
fn test_await_nested_future() {
    check_expr_type(
        "
let f future<future<string>>
await await f
",
        type_string(),
    );
}

#[test]
fn test_await_non_future_error() {
    check_error(
        "
let x = 10
await x
",
        "Await requires a Future",
    );
}

#[test]
fn test_await_in_expression() {
    check_expr_type(
        "
let f future<int>
(await f) + 1
",
        type_int(),
    );
}

#[test]
fn test_await_function_call() {
    check_expr_type(
        "
fn get_future() future<int>
    let f future<int>
    return f

await get_future()
",
        type_int(),
    );
}

#[test]
fn test_await_list_future() {
    check_expr_type(
        "
let f future<[int]>
await f
",
        type_list(type_int()),
    );
}

#[test]
fn test_await_map_future() {
    check_expr_type(
        "
let f future<map<string, int>>
await f
",
        type_map(type_string(), type_int()),
    );
}

#[test]
fn test_await_void_future() {
    check_expr_type(
        "
let f future<void>
await f
",
        type_custom("void", None),
    );
}

#[test]
fn test_await_nullable_future() {
    check_expr_type(
        "
let f future<int?>
await f
",
        type_null(type_int()),
    );
}

#[test]
fn test_await_custom_type() {
    check_expr_type(
        "
struct User
    name string

let f future<User>
await f
",
        type_custom("User", None),
    );
}

// #[test]
// fn test_await_generic_function() {
//     check_expr_type(
//         "
// fn unwrap<T>(f future<T>) T
//     return await f

// let x future<int>
// await unwrap<int>(x)
// ",
//         type_int(),
//     );
// }

#[test]
fn test_await_in_conditional_expression() {
    check_expr_type(
        "
let f future<bool>
1 if await f else 0
",
        type_int(),
    );
}

#[test]
fn test_await_in_loop_condition() {
    check_success(
        "
let f future<bool>
while await f
    break
",
    );
}
