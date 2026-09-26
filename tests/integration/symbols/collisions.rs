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

/// Kernel-side names join a definition's parts with `_`, so two static
/// methods whose owner and method names run together are declared under one
/// WGSL name when both are called from GPU code: the program is refused at
/// build time rather than at kernel launch.
#[test]
fn static_methods_spelled_alike_in_gpu_code_are_refused() {
    assert_build_error(
        r#"
use system.gpu
use system.collections.array

class A
    static fn b_c(x int) int
        return x + 1

class A_b
    static fn c(x int) int
        return x + 100

fn main()
    gpu let src = [1, 2, 3]
    gpu var dst = [0, 0, 0]
    gpu forall i in 0..3
        dst[i] = A.b_c(src[i]) * 1000 + A_b.c(src[i])
    let host = dst
    println(f'{host[0]}')
"#,
        "`A.b_c` and `A_b.c` are both reached from GPU code, where both are declared as `A_b_c`",
    );
}

/// A static method and a function spelled like its owner and name joined by
/// `_` share a WGSL name when a kernel calls both.
#[test]
fn static_method_and_function_spelled_alike_in_gpu_code_are_refused() {
    assert_build_error(
        r#"
use system.io

class P
    static fn norm(x int) int
        return x + 1

fn P_norm(x int) int
    return x + 2

fn main()
    gpu var a = [1, 2, 3, 4]
    gpu forall i in 0..4
        a[i] = P.norm(a[i]) + P_norm(a[i])
    let h = a
    println(f"{h[0]}")
"#,
        "MER_MIR_018",
    );
}

/// The same two definitions stay apart when only one of them is reached from
/// GPU code: the other is never declared under its WGSL name, and on the host
/// their link names differ.
#[test]
fn static_method_and_function_spelled_alike_run_apart_when_one_stays_on_the_host() {
    assert_runs_with_output(
        r#"
use system.io

class P
    static fn norm(x int) int
        return x + 1

fn P_norm(x int) int
    return x + 2

fn main()
    gpu var a = [1, 2, 3, 4]
    gpu forall i in 0..4
        a[i] = P_norm(a[i])
    let h = a
    println(f"{h[0]} {P.norm(1)} {P_norm(1)}")
"#,
        "3 2 3",
    );
}

/// A trait's default method and a free function spelled like the trait and
/// method joined by `_` each keep their own body.
#[test]
fn trait_default_method_beside_a_function_spelled_alike() {
    assert_runs_with_output(
        r#"
use system.io

trait Shape
    fn area() int
        return 1

class Square implements Shape
    side int

fn Shape_area(s Square) int
    return s.side * s.side

fn main()
    let s = Square(side: 3)
    println(f"{s.area()} {Shape_area(s)}")
"#,
        "1 9",
    );
}
