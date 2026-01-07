// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_function_declaration_and_call() {
    let source = "
fn add(a int, b int) int
    return a + b

add(1, 2)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_function_return_type_mismatch() {
    let source = "
fn foo() int
    return true
    ";
    check_error(source, "Invalid return type: expected int, got boolean");
}

#[test]
fn test_function_argument_type_mismatch() {
    let source = "
fn foo(a int)
    return

foo(true)
    ";
    check_error(
        source,
        "Type mismatch for argument 'a': expected int, got boolean",
    );
}

#[test]
fn test_function_argument_count_mismatch() {
    let source = "
fn foo(a int)
    return

foo(1, 2)
    ";
    check_error(source, "Too many positional arguments: expected 1, got 2");
}

#[test]
fn test_void_function() {
    let source = "
fn foo()
    return

foo()
    ";
    // Just check if it passes type checking
    check_success(source);
}

#[test]
fn test_nested_function_calls() {
    let source = "
fn add(a int, b int) int
    return a + b

add(add(1, 2), 3)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_recursion() {
    let source = "
fn factorial(n int) int
    if n <= 1: return 1
    return n * factorial(n - 1)

factorial(5)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_implicit_return_multiline() {
    let source = "
fn add(a int, b int) int
    let c = 10
    c * (a + b)

add(1, 2)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_implicit_return_inline() {
    let source = "
fn add(a int, b int) int: a + b

add(1, 2)
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_implicit_return_void_ignored() {
    let source = "
fn dummy_add(a int, b int)
   a + b

dummy_add(1, 2)
    ";
    check_success(source);
}

#[test]
fn test_void_function_explicit_return_value_error() {
    let source = "
fn dummy_add(a int, b int)
   return a + b
    ";
    check_error(source, "Invalid return type: expected void, got int");
}

#[test]
fn test_implicit_return_type_mismatch() {
    let source = "
fn foo() int
    true
    ";
    check_error(source, "Invalid return type: expected int, got boolean");
}

#[test]
fn test_implicit_return_block_scope() {
    let source = "
fn foo() int
    let a = 1
    a

foo()
    ";
    check_expr_type(source, type_int());
}

#[test]
fn test_default_argument_valid() {
    check_success(
        "
fn foo(a int = 1)
    return
foo()
foo(2)
",
    );
}

#[test]
fn test_default_argument_type_mismatch() {
    check_error(
        "
fn foo(a int = true)
    return
",
        "Type mismatch for default value",
    );
}

#[test]
fn test_guard_valid() {
    check_success(
        "
fn foo(a int > 0)
    return
foo(1)
",
    );
}

#[test]
fn test_guard_type_mismatch() {
    check_error(
        "
fn foo(a int > \"0\")
    return
",
        "Type mismatch",
    );
}

#[test]
fn test_parameter_shadowing() {
    check_success(
        "
let a = \"global\"
fn foo(a int) int
    return a
foo(1)
",
    );
}

#[test]
fn test_local_shadowing_parameter() {
    check_success(
        "
fn foo(a int) int
    let a = 2
    return a
foo(1)
",
    );
}

#[test]
fn test_generic_function_inference() {
    check_expr_type(
        "
fn id<T>(x T) T
    return x

id(1)
",
        type_int(),
    );
}

#[test]
fn test_higher_order_function() {
    check_expr_type(
        "
fn apply(f fn(int) int, x int) int
    return f(x)

fn square(x int) int
    return x * x

apply(square, 5)
",
        type_int(),
    );
}

#[test]
fn test_returning_function() {
    check_success(
        "
fn get_adder() fn(int) int
    return fn(x int): x + 1

let add = get_adder()
add(1)
",
    );
}

#[test]
fn test_guard_in_range() {
    check_success(
        "
fn foo(a int in 1..10)
    return
foo(5)
",
    );
}

#[test]
fn test_guard_in_list() {
    check_success(
        "
fn foo(a int in [1, 2, 3])
    return
foo(1)
",
    );
}

#[test]
fn test_guard_not() {
    check_success(
        "
fn foo(a int not 0)
    return
foo(1)
",
    );
}

#[test]
fn test_guard_referencing_previous_param() {
    check_success(
        "
fn foo(a int, b int > a)
    return
foo(1, 2)
",
    );
}

#[test]
fn test_default_value_referencing_previous_param() {
    check_success(
        "
fn foo(a int, b int = a)
    return
foo(1)
",
    );
}

#[test]
fn test_complex_generic_param() {
    check_expr_type(
        "
fn first<T>(list [T]) T
    return list[0]

first([1, 2, 3])
",
        type_int(),
    );
}

#[test]
fn test_nested_generic_param() {
    check_expr_type(
        "
fn flatten<T>(list [[T]]) [T]
    return list[0]

flatten([[1], [2]])
",
        type_list(type_int()),
    );
}

#[test]
fn test_map_generic_param() {
    check_expr_type(
        "
fn get_value<K, V>(map {K: V}, key K) V
    return map[key]

get_value({\"a\": 1}, \"a\")
",
        type_int(),
    );
}

#[test]
fn test_guard_type_mismatch_in() {
    check_error(
        "
fn foo(a int in [\"string\"])
    return
",
        "Type mismatch",
    );
}

#[test]
fn test_function_call_named_params() {
    let code = "
fn add(a int, b int) int
    return a + b

add(a: 1, b: 2)
    ";
    check_success(code);
}

#[test]
fn test_function_call_named_params_reordered() {
    let code = "
fn add(a int, b int) int
    return a + b

add(b: 2, a: 1)
    ";
    check_success(code);
}

#[test]
fn test_function_call_mixed_params() {
    let code = "
fn add(a int, b int, c int) int
    return a + b + c

add(1, c: 3, b: 2)
    ";
    check_success(code);
}

#[test]
fn test_function_call_unknown_param() {
    let code = "
fn add(a int)
    return

add(b: 1)
    ";
    check_error(code, "Unknown argument 'b'");
}

#[test]
fn test_missing_return_in_function() {
    let source = "
fn foo() int
    let x = 1
";
    check_error(source, "Missing return statement");
}

#[test]
fn test_missing_return_in_function_with_if() {
    let source = "
fn foo() int
    if true
        return 1
    
    // Missing return here
";
    check_error(source, "Missing return statement");
}

#[test]
fn test_gpu_function_must_return_void() {
    check_error(
        "gpu fn my_kernel() int: 1",
        "GPU functions must not have an explicit return type",
    );
}

#[test]
fn test_gpu_function_implicit_return() {
    let input = "
gpu fn my_kernel()
    let x = 1
";
    check_success(input);
}

#[test]
fn test_gpu_function_cannot_have_explicit_return() {
    let input = "
gpu fn my_kernel() void
    return
";
    check_error(input, "GPU functions must not have an explicit return type");
}

#[test]
fn test_gpu_function_cannot_call_print() {
    let input = "
gpu fn my_kernel()
    print(1)
";
    check_error(
        input,
        "Host function 'print' cannot be called from a GPU kernel",
    );
}

#[test]
fn test_gpu_function_can_use_builtins() {
    let input = "
gpu fn my_kernel()
    let x = gpu_context.thread_idx.x
";
    check_success(input);
}

#[test]
fn test_gpu_function_block_dim() {
    let input = "
gpu fn my_kernel()
    let x = gpu_context.block_dim.x
";
    check_success(input);
}
