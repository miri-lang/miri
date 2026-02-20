// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;

#[test]
fn test_function_declaration_and_call() {
    let source = "
fn add(a int, b int) int
    return a + b

add(1, 2)
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_function_return_type_mismatch() {
    let source = "
fn foo() int
    return true
    ";
    type_checker_error_test(source, "Invalid return type: expected int, got bool");
}

#[test]
fn test_function_argument_type_mismatch() {
    let source = "
fn foo(a int)
    return

foo(true)
    ";
    type_checker_error_test(
        source,
        "Type mismatch for argument 'a': expected int, got bool",
    );
}

#[test]
fn test_function_argument_count_mismatch() {
    let source = "
fn foo(a int)
    return

foo(1, 2)
    ";
    type_checker_error_test(source, "Too many positional arguments: expected 1, got 2");
}

#[test]
fn test_void_function() {
    let source = "
fn foo()
    return

foo()
    ";
    // Just check if it passes type checking
    type_checker_test(source);
}

#[test]
fn test_nested_function_calls() {
    let source = "
fn add(a int, b int) int
    return a + b

add(add(1, 2), 3)
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_recursion() {
    let source = "
fn factorial(n int) int
    if n <= 1: return 1
    return n * factorial(n - 1)

factorial(5)
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_implicit_return_multiline() {
    let source = "
fn add(a int, b int) int
    let c = 10
    c * (a + b)

add(1, 2)
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_implicit_return_inline() {
    let source = "
fn add(a int, b int) int: a + b

add(1, 2)
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_implicit_return_void_ignored() {
    let source = "
fn dummy_add(a int, b int)
   a + b

dummy_add(1, 2)
    ";
    type_checker_test(source);
}

#[test]
fn test_void_function_explicit_return_value_error() {
    let source = "
fn dummy_add(a int, b int)
   return a + b
    ";
    type_checker_error_test(source, "Invalid return type: expected void, got int");
}

#[test]
fn test_implicit_return_type_mismatch() {
    let source = "
fn foo() int
    true
    ";
    type_checker_error_test(source, "Invalid return type: expected int, got bool");
}

#[test]
fn test_implicit_return_block_scope() {
    let source = "
fn foo() int
    let a = 1
    a

foo()
    ";
    type_checker_expr_type_test(source, type_int());
}

#[test]
fn test_default_argument_valid() {
    type_checker_test(
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
    type_checker_error_test(
        "
fn foo(a int = true)
    return
",
        "Type mismatch for default value",
    );
}

#[test]
fn test_guard_valid() {
    type_checker_test(
        "
fn foo(a int > 0)
    return
foo(1)
",
    );
}

#[test]
fn test_guard_type_mismatch() {
    type_checker_error_test(
        "
fn foo(a int > \"0\")
    return
",
        "Type mismatch",
    );
}

#[test]
fn test_parameter_shadowing() {
    type_checker_test(
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
    type_checker_test(
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
    type_checker_expr_type_test(
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
    type_checker_expr_type_test(
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
    type_checker_test(
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
    type_checker_test(
        "
fn foo(a int in 1..10)
    return
foo(5)
",
    );
}

#[test]
fn test_guard_in_list() {
    type_checker_test(
        "
fn foo(a int in [1, 2, 3])
    return
foo(1)
",
    );
}

#[test]
fn test_guard_not() {
    type_checker_test(
        "
fn foo(a int not 0)
    return
foo(1)
",
    );
}

#[test]
fn test_guard_referencing_previous_param() {
    type_checker_test(
        "
fn foo(a int, b int > a)
    return
foo(1, 2)
",
    );
}

#[test]
fn test_default_value_referencing_previous_param() {
    type_checker_test(
        "
fn foo(a int, b int = a)
    return
foo(1)
",
    );
}

#[test]
fn test_complex_generic_param() {
    type_checker_expr_type_test(
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
    type_checker_expr_type_test(
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
    type_checker_expr_type_test(
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
    type_checker_error_test(
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
    type_checker_test(code);
}

#[test]
fn test_function_call_named_params_reordered() {
    let code = "
fn add(a int, b int) int
    return a + b

add(b: 2, a: 1)
    ";
    type_checker_test(code);
}

#[test]
fn test_function_call_mixed_params() {
    let code = "
fn add(a int, b int, c int) int
    return a + b + c

add(1, c: 3, b: 2)
    ";
    type_checker_test(code);
}

#[test]
fn test_function_call_unknown_param() {
    let code = "
fn add(a int)
    return

add(b: 1)
    ";
    type_checker_error_test(code, "Unknown argument 'b'");
}

#[test]
fn test_missing_return_in_function() {
    let source = "
fn foo() int
    let x = 1
";
    type_checker_error_test(source, "Missing return statement");
}

#[test]
fn test_missing_return_in_function_with_if() {
    let source = "
fn foo() int
    if true
        return 1
    
    // Missing return here
";
    type_checker_error_test(source, "Missing return statement");
}

#[test]
fn test_gpu_function_must_return_void() {
    type_checker_error_test(
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
    type_checker_test(input);
}

#[test]
fn test_gpu_function_cannot_have_explicit_return() {
    let input = "
gpu fn my_kernel() void
    return
";
    type_checker_error_test(input, "GPU functions must not have an explicit return type");
}

#[test]
fn test_gpu_function_cannot_call_print() {
    let input = "
gpu fn my_kernel()
    print(1)
";
    type_checker_error_test(
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
    type_checker_test(input);
}

#[test]
fn test_gpu_function_block_dim() {
    let input = "
gpu fn my_kernel()
    let x = gpu_context.block_dim.x
";
    type_checker_test(input);
}

#[test]
fn test_function_deeply_nested_calls() {
    type_checker_expr_type_test(
        "
fn add(a int, b int) int: a + b

add(add(add(add(add(1, 2), 3), 4), 5), 6)
",
        type_int(),
    );
}

#[test]
fn test_function_many_parameters() {
    type_checker_test(
        "
fn many(a int, b int, c int, d int, e int, f int, g int, h int, i int, j int) int
    a + b + c + d + e + f + g + h + i + j

many(1, 2, 3, 4, 5, 6, 7, 8, 9, 10)
",
    );
}

#[test]
fn test_function_chain_composition() {
    type_checker_expr_type_test(
        "
fn double(x int) int: x * 2
fn triple(x int) int: x * 3
fn add_one(x int) int: x + 1

add_one(triple(double(add_one(double(1)))))
",
        type_int(),
    );
}

#[test]
fn test_function_mutual_recursion() {
    // TODO: Feature not implemented - mutual recursion requires forward declarations
    type_checker_error_test(
        "
fn is_even(n int) bool
    if n == 0: return true
    return is_odd(n - 1)

fn is_odd(n int) bool
    if n == 0: return false
    return is_even(n - 1)

is_even(10)
",
        "Undefined variable: is_odd",
    );
}

#[test]
fn test_function_all_named_params_reordered() {
    type_checker_test(
        "
fn point(x int, y int, z int) int: x + y + z

point(z: 3, y: 2, x: 1)
",
    );
}

#[test]
fn test_function_many_default_params() {
    type_checker_test(
        "
fn defaults(a int = 1, b int = 2, c int = 3, d int = 4, e int = 5) int
    a + b + c + d + e

defaults()
defaults(10)
defaults(10, 20)
defaults(10, 20, 30, 40, 50)
",
    );
}

#[test]
fn test_function_nested_lambdas() {
    type_checker_test(
        "
let f = fn(x int) fn(int) int
    return fn(y int): x + y

let add5 = f(5)
add5(10)
",
    );
}

#[test]
fn test_function_long_body_chain() {
    type_checker_test(
        "
fn compute(x int) int
    let a = x + 1
    let b = a * 2
    let c = b - 3
    let d = c / 2
    let e = d + 100
    let f = e * e
    let g = f - 1
    let h = g + x
    h

compute(5)
",
    );
}

#[test]
fn test_function_generic_with_multiple_constraints() {
    type_checker_test(
        "
fn identity<T>(x T) T: x

identity(1)
identity(\"hello\")
identity(true)
identity([1, 2, 3])
",
    );
}

#[test]
fn test_function_error_many_wrong_types() {
    type_checker_error_test(
        "
fn foo(a int, b int) int: a + b
foo(\"a\", \"b\")
",
        "Type mismatch for argument 'a'",
    );
}

#[test]
fn test_function_error_extra_args() {
    type_checker_error_test(
        "
fn foo(a int) int: a
foo(1, 2, 3, 4, 5)
",
        "Too many positional arguments",
    );
}

#[test]
fn test_function_inline_expression_body() {
    type_checker_exprs_type_test(vec![
        ("fn id(x int) int: x\nid(42)", type_int()),
        ("fn neg(x bool) bool: not x\nneg(true)", type_bool()),
    ]);
}
