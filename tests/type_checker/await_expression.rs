// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use super::utils::{check_error, check_expr_type, check_success};
use miri::{ast::factory::typ, ast::Type};

#[test]
fn test_await_future_variable() {
    check_expr_type(
        "
let f future<int>
await f
",
        Type::Int,
    );
}

#[test]
fn test_await_nested_future() {
    check_expr_type(
        "
let f future<future<string>>
await await f
",
        Type::String,
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
        Type::Int,
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
        Type::Int,
    );
}

#[test]
fn test_await_list_future() {
    check_expr_type(
        "
let f future<[int]>
await f
",
        Type::List(Box::new(typ(Type::Int))),
    );
}

#[test]
fn test_await_map_future() {
    check_expr_type(
        "
let f future<map<string, int>>
await f
",
        Type::Map(Box::new(typ(Type::String)), Box::new(typ(Type::Int))),
    );
}

#[test]
fn test_await_void_future() {
    check_expr_type(
        "
let f future<void>
await f
",
        Type::Custom("void".to_string(), None),
    );
}

#[test]
fn test_await_nullable_future() {
    check_expr_type(
        "
let f future<int?>
await f
",
        Type::Nullable(Box::new(Type::Int)),
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
        Type::Custom("User".to_string(), None),
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
//         Type::Int,
//     );
// }

#[test]
fn test_await_in_conditional_expression() {
    check_expr_type(
        "
let f future<bool>
1 if await f else 0
",
        Type::Int,
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
