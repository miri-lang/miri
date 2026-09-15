// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic body's ordering requirement binds every site that pins it, wherever
//! in the source that site is written: above the body, below it, or inside
//! another generic body declared before the one it delegates to.

use super::utils::*;

#[test]
fn test_a_call_written_above_an_ordering_body_refuses_an_unordered_type() {
    assert_compiler_error(
        r#"
struct P
    v int

fn main()
    let won = smaller(P(1), P(2))
    println(f"{won.v}")

fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b
"#,
        "'smaller' orders its 'T' parameter",
    );
}

#[test]
fn test_a_delegating_body_declared_above_its_delegate_refuses_an_unordered_type() {
    assert_compiler_error(
        r#"
struct P
    v int

fn main()
    let r = outer(P(1), P(2))
    println(f"{r.v}")

fn outer<U>(a U, b U) U
    return smaller(a, b)

fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b
"#,
        "'outer' orders its 'U' parameter",
    );
}

#[test]
fn test_a_chain_of_delegates_declared_top_down_names_the_outermost_body() {
    assert_compiler_error(
        r#"
struct P
    v int

fn main()
    let r = top(P(1), P(2))
    println(f"{r.v}")

fn top<V>(a V, b V) V
    return middle(a, b)

fn middle<U>(a U, b U) U
    return smaller(a, b)

fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b
"#,
        "'top' orders its 'V' parameter",
    );
}

#[test]
fn test_a_method_call_written_above_its_class_refuses_an_unordered_type() {
    assert_compiler_error(
        r#"
struct P
    v int

fn main()
    let box = Box<P>(P(1))
    let r = box.pick(P(2))
    println(f"{r.v}")

class Box<T>
    v T

    fn init(v T)
        self.v = v

    public fn pick(other T) T
        if self.v < other
            return self.v
        return other
"#,
        "'pick' orders its 'T' parameter",
    );
}

#[test]
fn test_a_method_delegating_to_a_function_below_it_refuses_an_unordered_type() {
    assert_compiler_error(
        r#"
struct P
    v int

fn main()
    let box = Box<P>(P(1))
    let r = box.pick(P(2))
    println(f"{r.v}")

class Box<T>
    v T

    fn init(v T)
        self.v = v

    public fn pick(other T) T
        return smaller(self.v, other)

fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b
"#,
        "'pick' orders its 'T' parameter",
    );
}

#[test]
fn test_bodies_declared_top_down_still_order_an_ordered_type_by_content() {
    assert_runs_with_output(
        r#"
fn main()
    println(top("PEAR".to_lower(), "APPLE".to_lower()))
    println(f"{top(9, 4)}")

fn top<V>(a V, b V) V
    return smaller(a, b)

fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b
"#,
        "apple\n4",
    );
}

#[test]
fn test_a_delegate_below_that_orders_nothing_accepts_an_unordered_type() {
    assert_runs_with_output(
        r#"
struct P
    v int

fn main()
    let r = outer(P(8), P(9))
    println(f"{r.v}")

fn outer<U>(a U, b U) U
    return keep(a, b)

fn keep<T>(a T, _b T) T
    return a
"#,
        "8",
    );
}

#[test]
fn test_mutually_recursive_generic_bodies_settle_their_requirement() {
    // Each body delegates to the other, so the requirement written in `even`
    // reaches `odd` only by going around the cycle; settling must terminate.
    assert_compiler_error(
        r#"
struct P
    v int

fn main()
    let r = odd(P(1), P(2), 3)
    println(f"{r.v}")

fn odd<U>(a U, b U, n int) U
    if n == 0
        return a
    return even(a, b, n - 1)

fn even<T>(a T, b T, n int) T
    if n == 0
        if a < b
            return a
        return b
    return odd(a, b, n - 1)
"#,
        "'odd' orders its 'U' parameter",
    );
}
