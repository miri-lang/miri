// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Definitions whose identifiers run together, or that share a name with a
//! symbol the compiler synthesizes or a function the runtime or C library
//! exports. Each is compiled under a name of its own, so every call runs the
//! body it names.

use super::utils::*;
use crate::utils::miri_run;

#[test]
fn generic_instance_beside_a_function_spelled_alike() {
    assert_runs_with_output(
        r#"
use system.io

fn pick<T>(x T) T
    return x

fn pick__int(x int) int
    return x + 1000

fn main()
    println(f"{pick<int>(7)} {pick__int(7)}")
    println(f"{pick(5)}")
"#,
        "7 1007\n5",
    );
}

#[test]
fn function_declared_before_a_generic_instance_spelled_alike() {
    assert_runs_with_output(
        r#"
use system.io

fn pick__int(x int) int
    return x + 1000

fn pick<T>(x T) T
    return x

fn main()
    println(f"{pick(5)} {pick__int(7)}")
"#,
        "5 1007",
    );
}

#[test]
fn generic_instance_beside_a_static_method_spelled_alike() {
    assert_runs_with_output(
        r#"
use system.io

class Pick
    public static fn _int(x int) int
        return x + 1000

fn Pick<T>(x T) T
    return x

fn main()
    println(f"{Pick<int>(7)}")
"#,
        "7",
    );
}

#[test]
fn methods_of_classes_whose_names_run_together() {
    assert_runs_with_output(
        r#"
use system.io

class A_b
    fn c() int
        return 1

class A
    fn b_c() int
        return 2

fn main()
    let x = A_b()
    let y = A()
    println(f"{x.c()} {y.b_c()}")
"#,
        "1 2",
    );
}

#[test]
fn function_declared_before_a_method_spelled_alike() {
    assert_runs_with_output(
        r#"
use system.io

fn Point_norm(_p Point) int
    return 2

class Point
    x int
    fn norm() int
        return 1

fn main()
    let p = Point(x: 1)
    println(f"{p.norm()} {Point_norm(p)}")
"#,
        "1 2",
    );
}

#[test]
fn function_declared_after_a_method_spelled_alike() {
    assert_runs_with_output(
        r#"
use system.io

class Point
    fn norm() int
        return 1

fn Point_norm(p Point) int
    return 2

fn main()
    let p = Point()
    println(f"{p.norm()} {Point_norm(p)}")
"#,
        "1 2",
    );
}

#[test]
fn generic_class_instance_beside_a_class_spelled_alike() {
    assert_runs_with_output(
        r#"
use system.io

class W<T>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v

class W__int
    s String
    fn init(s String)
        self.s = s
    fn get() String
        return self.s

fn main()
    let a = W<int>(7)
    let b = W__int("x" + "y")
    println(f"{a.get()} {b.get()}")
"#,
        "7 xy",
    );
}

#[test]
fn function_spelled_like_a_drop_function() {
    assert_runs_with_output(
        r#"
use system.io

class Point
    x int

fn __drop_Point(p int) int
    return p

fn main()
    let p = Point(x: 3)
    println(f"{p.x} {__drop_Point(4)}")
"#,
        "3 4",
    );
}

#[test]
fn function_named_like_a_c_library_function_the_runtime_calls() {
    assert_runs_with_output(
        r#"
use system.io

fn write(fd int, buf int, n int) int
    return fd + buf + n

fn main()
    println("hello")
    println(f"{write(1, 2, 3)}")
"#,
        "hello\n6",
    );
}

#[test]
fn function_named_malloc_beside_string_allocation() {
    assert_runs_with_output(
        r#"
use system.io

fn malloc(n int) int
    return n + 1

fn main()
    println(f"{malloc(1)}")
    let s = "abc" + "def"
    println(s)
"#,
        "2\nabcdef",
    );
}

#[test]
fn function_named_free_is_never_called_by_the_runtime() {
    let result = miri_run(
        r#"
use system.io

fn free(p int)
    println(f"user free called with {p}")

fn main()
    let a = "ab"
    let s = a + "cd"
    println(s)
"#,
    );
    let output = result.output();
    assert!(result.success, "program failed:\n{output}");
    assert!(output.contains("abcd"), "missing output:\n{output}");
    assert!(
        !output.contains("user free called"),
        "the runtime released memory through the program's `free`:\n{output}"
    );
}

#[test]
fn function_named_like_a_runtime_function_the_stdlib_declares() {
    assert_runs_with_output(
        r#"
use system.io

fn miri_rt_string_concat(a int, b int) int
    return a * 100 + b

fn main()
    let a = "ab"
    let s = a + "cd"
    println(s)
    println(f"{miri_rt_string_concat(3, 4)}")
"#,
        "abcd\n304",
    );
}

#[test]
fn reference_to_a_function_named_like_a_runtime_function() {
    assert_runs_with_output(
        r#"
use system.io

fn miri_rt_string_concat(a int, b int) int
    return a * 100 + b

fn apply(f fn(int, int) int) int
    return f(5, 6)

fn main()
    let s = "ab" + "cd"
    println(s)
    println(f"{apply(miri_rt_string_concat)}")
"#,
        "abcd\n506",
    );
}
