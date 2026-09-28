// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests that verify the type checker catches argument-count and type
//! mismatches when calling regular functions.

use super::utils::*;

// ── Too few arguments ─────────────────────────────────────────────────────────

#[test]
fn test_function_too_few_args() {
    assert_compiler_error(
        r#"
fn add(a int, b int) int
    a + b

fn main()
    let _ = add(1)
    "#,
        "Missing argument for parameter 'b'",
    );
}

#[test]
fn test_function_no_args_when_params_required() {
    assert_compiler_error(
        r#"
fn greet(name String)
    ()

fn main()
    greet()
    "#,
        "Missing argument for parameter 'name'",
    );
}

// ── Too many arguments ────────────────────────────────────────────────────────

#[test]
fn test_function_too_many_positional_args() {
    assert_compiler_error(
        r#"
fn double(x int) int
    x * 2

fn main()
    let _ = double(3, 4)
    "#,
        "Too many positional arguments",
    );
}

#[test]
fn test_function_args_to_no_param_function() {
    assert_compiler_error(
        r#"
fn noop()
    ()

fn main()
    noop(99)
    "#,
        "Too many positional arguments",
    );
}

// ── Wrong argument types ──────────────────────────────────────────────────────

#[test]
fn test_function_wrong_arg_type() {
    assert_compiler_error(
        r#"
fn square(n int) int
    n * n

fn main()
    let _ = square("hello")
    "#,
        "Type mismatch for argument 'n'",
    );
}

#[test]
fn test_function_wrong_second_arg_type() {
    assert_compiler_error(
        r#"
fn concat(a String, b String) String
    a

fn main()
    let _ = concat("hi", 42)
    "#,
        "Type mismatch for argument 'b'",
    );
}

// ── Unknown named argument ────────────────────────────────────────────────────

#[test]
fn test_function_unknown_named_arg() {
    assert_compiler_error(
        r#"
fn inc(n int) int
    n + 1

fn main()
    let _ = inc(x: 5)
    "#,
        "Unknown argument 'x'",
    );
}

// ── Valid calls must still compile ───────────────────────────────────────────

#[test]
fn test_function_correct_call() {
    assert_runs_with_output(
        r#"
fn add(a int, b int) int
    a + b

fn main()
    println(f"{add(3, 4)}")
    "#,
        "7",
    );
}

#[test]
fn test_function_named_args_reordered() {
    assert_runs_with_output(
        r#"
fn sub(a int, b int) int
    a - b

fn main()
    println(f"result={sub(b: 2, a: 10)}")
    "#,
        "result=8",
    );
}

#[test]
fn test_function_named_args_out_of_order_bind_by_name() {
    assert_runs_with_output(
        r#"
fn f(a int, b int)
    println(f"a={a} b={b}")

fn main()
    f(b: 2, a: 1)
    "#,
        "a=1 b=2",
    );
}

#[test]
fn test_function_positional_then_named_args_out_of_order_bind_by_name() {
    assert_runs_with_output(
        r#"
fn f(a int, b int, c int)
    println(f"a={a} b={b} c={c}")

fn main()
    f(1, c: 3, b: 2)
    "#,
        "a=1 b=2 c=3",
    );
}

#[test]
fn test_function_omitted_default_is_supplied_between_named_args() {
    assert_runs_with_output(
        r#"
fn g(a int, b int = 7, c int = 9)
    println(f"a={a} b={b} c={c}")

fn main()
    g(c: 3, a: 1)
    "#,
        "a=1 b=7 c=3",
    );
}

#[test]
fn test_function_named_args_are_evaluated_in_written_order() {
    assert_runs_with_output(
        r#"
fn trace(x int) int
    println(f"eval {x}")
    return x

fn f(a int, b int)
    println(f"a={a} b={b}")

fn main()
    f(b: trace(2), a: trace(1))
    "#,
        "eval 2\neval 1\na=1 b=2",
    );
}

#[test]
fn test_function_managed_named_args_out_of_order_bind_by_name() {
    assert_runs_with_output(
        r#"
fn f(a String, b String)
    println(f"a={a} b={b}")

fn main()
    let x = "one"
    f(b: f"{x}-two", a: f"{x}-one")
    "#,
        "a=one-one b=one-two",
    );
}

#[test]
fn test_generic_function_named_args_out_of_order_bind_by_name() {
    assert_runs_with_output(
        r#"
fn first<T>(a T, b T) T
    return a

fn main()
    println(f"first={first(b: 2, a: 1)}")
    "#,
        "first=1",
    );
}

#[test]
fn test_function_named_arg_duplicating_a_positional_one_is_refused() {
    assert_compiler_error(
        r#"
fn f(a int, b int)
    println(f"a={a} b={b}")

fn main()
    f(1, a: 2)
    "#,
        "Argument 'a' is already given positionally",
    );
}

#[test]
fn test_function_default_param_can_be_omitted() {
    assert_runs_with_output(
        r#"
fn greet(name String, prefix String = "Hello") String
    f"{prefix}, {name}"

fn main()
    println(greet("Alice"))
    "#,
        "Hello, Alice",
    );
}
