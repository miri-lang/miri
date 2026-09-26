// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An instance a body builds at arguments computed from its own — a value
//! argument folded from a `const`, a closure or `Result` type argument — is
//! compiled at the arguments it names, and one whose arguments name no single
//! instantiation is refused rather than run through the body shared by every
//! instantiation.

use super::utils::*;

/// A value argument computed from a parameter and a `const` names one
/// instantiation once the body is compiled at a concrete size: the grown
/// instance answers through its own body, which reads a float at its width.
#[test]
fn test_a_value_argument_computed_with_a_const_answers_with_its_own_body() {
    assert_heap_guard_output(
        r#"
use system.io

const K = 1

trait Op<T>
    fn get() T

class Buf<T, Size> implements Op<T>
    v T
    fn get() T
        return self.v
    fn grow() Op<T>
        return Buf<T, Size + K>(v: self.v)

fn main()
    let b = Buf<float, 1>(v: 2.5)
    let g = b.grow()
    println(f"{g.get()} {b.get()}")
"#,
        "2.5 2.5",
    );
}

/// A method growing its class's value argument by a `const` on every static
/// call needs a new instantiation per call, and is refused as polymorphic
/// recursion rather than compiled through the shared body.
#[test]
fn test_a_value_argument_grown_by_a_const_on_every_call_is_refused() {
    assert_build_error(
        r#"
use system.io

const K = 1

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn get(n int) T
        if n == 0
            return self.v
        let b = Buf<T, Size + K>(self.v)
        return b.get(n - 1)

fn main()
    let o = Buf<String, 2>("a" + "b")
    println(o.get(1))
"#,
        "MER_MIR_016",
    );
}

/// A class instantiated at a closure type answers through a trait receiver
/// with the body compiled at that closure type, which counts the closure it
/// overwrites and the one it returns.
#[test]
fn test_a_closure_type_argument_behind_a_trait_answers_with_its_own_body() {
    assert_heap_guard_output(
        r#"
use system.io

trait Op<T>
    fn keep(a T, b T) T

class Impl<T> implements Op<T>
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

fn main()
    let s = "a" + "b"
    let f = fn() String: s + "c"
    let g = fn() String: s + "d"
    let o Op<fn() String> = Impl<fn() String>()
    let h = o.keep(f, g)
    println(h())
"#,
        "abd",
    );
}

/// A class instantiated at a closure type stores the closure its `init` is
/// handed and returns it from its own body.
#[test]
fn test_a_closure_type_argument_is_stored_and_returned_by_its_own_body() {
    assert_heap_guard_output(
        r#"
use system.io

class W<T>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v

fn main()
    let s = "a" + "b"
    let w = W<fn() String>(fn() String: s + "c")
    let h = w.get()
    println(h())
"#,
        "abc",
    );
}

/// Two closure types of different shapes name two instantiations of one class.
#[test]
fn test_two_closure_type_arguments_name_two_instantiations() {
    assert_heap_guard_output(
        r#"
use system.io

class W<T>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v

fn main()
    let s = "a" + "b"
    let f = W<fn() String>(fn() String: s + "c")
    let g = W<fn(x int) int>(fn(x int) int: x + 1)
    let ff = f.get()
    let gg = g.get()
    println(ff())
    println(f"{gg(4)}")
"#,
        "abc\n5",
    );
}

/// A value argument that overflows once its parameter is bound names no
/// instantiation: the program is refused rather than run through the body
/// shared by every instantiation.
#[test]
fn test_a_value_argument_overflowing_at_its_instantiation_is_refused() {
    assert_build_error(
        r#"
use system.io

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn doubled() int
        let b = Buf<T, Size * 2>(self.v)
        return 1

fn main()
    let b = Buf<String, 85070591730234615865843651857942052864>("a" + "b")
    println(f"{b.doubled()}")
"#,
        "MER_MIR_017",
    );
}

/// The refusal names the instance as written, the value its parameter is
/// bound to, and why the argument has no value.
#[test]
fn test_an_overflowing_value_argument_reports_the_binding_and_the_cause() {
    assert_build_error(
        r#"
use system.io

trait Op<T>
    fn depth(n int) T

class Buf<T, Size> implements Op<T>
    v T
    fn init(v T)
        self.v = v
    fn depth(n int) T
        if n == 0
            return self.v
        let a Op<T> = Buf<T, Size * 2>(self.v)
        return a.depth(n - 1)

fn main()
    let x = "a" + "b"
    let o Op<String> = Buf<String, 85070591730234615865843651857942052864>(x)
    println(f"{o.depth(1)}")
"#,
        "instantiating `Buf<String, Size * 2>` at `Size = 85070591730234615865843651857942052864`: `Size * 2` does not fit in a 128-bit integer",
    );
}

/// A value argument that divides by zero once its parameter is bound is
/// refused the same way.
#[test]
fn test_a_value_argument_dividing_by_zero_at_its_instantiation_is_refused() {
    assert_build_error(
        r#"
use system.io

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn split() int
        let b = Buf<T, Size / (Size - 2)>(self.v)
        return 1

fn main()
    let b = Buf<String, 2>("a" + "b")
    println(f"{b.split()}")
"#,
        "`Size / (Size - 2)` divides by zero",
    );
}

/// A value argument past the signed 128-bit range, grown from a written
/// unsigned one, is refused.
#[test]
fn test_a_value_argument_grown_past_the_signed_range_is_refused() {
    assert_build_error(
        r#"
use system.io

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn grown() int
        let b = Buf<T, Size + 1>(self.v)
        return 1

fn main()
    let b = Buf<String, 170141183460469231731687303715884105728>("a" + "b")
    println(f"{b.grown()}")
"#,
        "MER_MIR_017",
    );
}

/// A value argument that stays in range, even a negative one, names its own
/// instantiation and runs its own body.
#[test]
fn test_a_value_argument_folding_below_zero_runs_its_own_body() {
    assert_heap_guard_output(
        r#"
use system.io

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v
    fn shrunk() T
        let b = Buf<T, Size - 3>(self.v)
        return b.get()

fn main()
    let b = Buf<String, 1>("a" + "b")
    println(b.shrunk())
"#,
        "ab",
    );
}

/// A generic function instantiated at a function taking an `out` parameter
/// builds its class at that function type, which names its own instantiation.
#[test]
fn test_a_function_type_with_an_out_parameter_names_its_own_instantiation() {
    assert_runs_with_output(
        r#"
use system.io

fn setter(x out int)
    x = 4

class W<T>
    v T
    fn init(v T)
        self.v = v
    fn size() int
        return 1

fn wrap<F>(f F) int
    let w = W<F>(f)
    return w.size()

fn main()
    println(f"{wrap(setter)}")
"#,
        "1",
    );
}

/// A declared generic whose name reads like a closure's token is a different
/// type from that closure: each instantiation keeps its own body, drop thunk
/// and accessors.
#[test]
fn test_a_struct_named_like_a_closure_token_and_the_closure_keep_two_instantiations() {
    assert_heap_guard_output(
        r#"
use system.io

struct fn1<A, B>
    a int
    b int

class W<T>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v

fn main()
    let b = W<fn1<int, int>>(fn1<int, int>(a: 40, b: 2))
    println(f"{b.get().a + b.get().b}")
    let s = "k" + "z"
    let g = fn(x int) int
        return x + s.length()
    let a = W<fn(x int) int>(g)
    let h = a.get()
    println(f"{h(1)}")
"#,
        "42\n3",
    );
}

/// Two tuples whose leaves flatten to the same sequence are two layouts, and
/// each instantiation reads its own.
#[test]
fn test_tuples_nested_differently_over_the_same_leaves_keep_two_instantiations() {
    assert_heap_guard_output(
        r#"
use system.io

class W<T>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v

fn main()
    let a = W<((int, int), String, int)>(((1, 2), "x" + "y", 3))
    let b = W<((int, int, String), int)>(((4, 5, "p" + "q"), 6))
    println(a.get().1)
    println(f"{b.get().1}")
    let inner = b.get().0
    println(inner.2)
"#,
        "xy\n6\npq",
    );
}

/// A value argument doubled on every call outgrows the integer range before
/// the class reaches its bound on instantiations; the refusal names the chain
/// that grew it, not only the last value.
#[test]
fn test_a_value_argument_doubled_on_every_call_is_refused_as_a_growing_chain() {
    assert_build_error(
        r#"
use system.io

trait Op<T>
    fn depth(n int) T

class Buf<T, Size> implements Op<T>
    v T
    fn init(v T)
        self.v = v
    fn depth(n int) T
        if n == 0
            return self.v
        let a Op<T> = Buf<T, Size * 2>(self.v)
        return a.depth(n - 1)

fn main()
    let o Op<String> = Buf<String, 2>("a" + "b")
    println(o.depth(3))
"#,
        "each instance of `Buf` builds the next at a new value of `Size`",
    );
}
