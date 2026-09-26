// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic body that calls another generic body hands its own instantiation on:
//! the delegate is compiled for the type the outermost call pins, not for the
//! parameter the delegating body was written against.
//!
//! Strings are built at run time so an address comparison or an unretained
//! reference cannot pass for the right answer.

use super::utils::*;

#[test]
fn test_a_struct_reaches_a_delegate_at_its_own_width() {
    assert_runs_with_output(
        r#"
struct P
    v int

fn keep<T>(a T, _b T) T
    return a

fn outer<T>(a T, b T) T
    return keep(a, b)

fn main()
    let r = outer(P(1), P(2))
    println(f"{r.v}")
"#,
        "1",
    );
}

#[test]
fn test_a_delegate_orders_strings_by_content() {
    assert_runs_with_output(
        r#"
fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

fn outer<T>(a T, b T) T
    return smaller(a, b)

fn main()
    println(outer("PEAR".to_lower(), "APPLE".to_lower()))
"#,
        "apple",
    );
}

#[test]
fn test_two_levels_of_delegation_reach_the_innermost_body() {
    assert_runs_with_output(
        r#"
struct P
    v int

fn keep<T>(a T, _b T) T
    return a

fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

fn middle<T>(a T, b T) T
    return smaller(a, b)

fn top<T>(a T, b T) T
    return middle(a, b)

fn kept_middle<T>(a T, b T) T
    return keep(a, b)

fn kept_top<T>(a T, b T) T
    return kept_middle(a, b)

fn main()
    println(top("PEAR".to_lower(), "APPLE".to_lower()))
    let r = kept_top(P(4), P(5))
    println(f"{r.v}")
"#,
        "apple\n4",
    );
}

#[test]
fn test_one_delegating_body_serves_each_instantiation_separately() {
    assert_runs_with_output(
        r#"
fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

fn outer<T>(a T, b T) T
    return smaller(a, b)

fn main()
    let n = outer(7, 3)
    let s = outer("PEAR".to_lower(), "APPLE".to_lower())
    let f = outer(2.5, 1.5)
    println(f"{n} {s} {f}")
"#,
        "3 apple 1.5",
    );
}

#[test]
fn test_a_direct_call_and_a_delegated_call_at_one_type_both_order_by_content() {
    // `middle(...)` records `U` at `String`, which spells the same symbol as
    // `smaller` at `String`; `smaller` must still be compiled with its own `T`
    // bound, whichever of the two calls is matched first.
    assert_runs_with_output(
        r#"
fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

fn middle<U>(a U, b U) U
    return smaller(a, b)

fn main()
    println(middle("PEAR".to_lower(), "APPLE".to_lower()))
    println(smaller("B".to_lower(), "A".to_lower()))
"#,
        "apple\na",
    );
}

#[test]
fn test_a_delegate_pinned_to_the_callers_element_type() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

fn least_of_two<T>(items List<T>) T
    let first = items[0]
    let second = items[1]
    return smaller(first, second)

fn main()
    var words = List<String>()
    words.push("PEAR".to_lower())
    words.push("APPLE".to_lower())
    println(least_of_two(words))
"#,
        "apple",
    );
}

#[test]
fn test_a_delegate_orders_a_user_type_by_its_own_rule() {
    assert_runs_with_output(
        r#"
use system.ops

class Weight implements Comparable
    grams int

    fn init(grams int)
        self.grams = grams

    public fn compare(other Weight) int
        return self.grams - other.grams

fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

fn outer<T>(a T, b T) T
    return smaller(a, b)

fn main()
    let won = outer(Weight(20), Weight(10))
    println(f"{won.grams}")
"#,
        "10",
    );
}

#[test]
fn test_an_unordered_type_pinned_through_a_delegate_is_refused() {
    assert_compiler_error(
        r#"
struct P
    v int

fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

fn outer<T>(a T, b T) T
    return smaller(a, b)

fn main()
    let r = outer(P(1), P(2))
    println(f"{r.v}")
"#,
        "MER_TYP_075",
    );
}

#[test]
fn test_an_unordered_type_pinned_through_two_delegates_names_the_outer_body() {
    assert_compiler_error(
        r#"
struct P
    v int

fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

fn middle<U>(a U, b U) U
    return smaller(a, b)

fn top<V>(a V, b V) V
    return middle(a, b)

fn main()
    let r = top(P(1), P(2))
    println(f"{r.v}")
"#,
        "'top' orders its 'V' parameter",
    );
}

#[test]
fn test_a_delegating_body_that_orders_nothing_accepts_an_unordered_type() {
    assert_runs_with_output(
        r#"
struct P
    v int

fn keep<T>(a T, _b T) T
    return a

fn outer<U>(a U, b U) U
    return keep(a, b)

fn main()
    let r = outer(P(8), P(9))
    println(f"{r.v}")
"#,
        "8",
    );
}

#[test]
fn test_a_generic_class_method_delegates_to_a_generic_function() {
    assert_runs_with_output(
        r#"
fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

class Box<T>
    v T

    fn init(v T)
        self.v = v

    public fn pick(other T) T
        return smaller(self.v, other)

fn main()
    let words = Box<String>("PEAR".to_lower())
    println(words.pick("APPLE".to_lower()))
    let numbers = Box<int>(9)
    println(f"{numbers.pick(4)}")
"#,
        "apple\n4",
    );
}

#[test]
fn test_an_unordered_type_pinned_through_a_method_that_delegates_is_refused() {
    assert_compiler_error(
        r#"
struct P
    v int

fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

class Box<T>
    v T

    fn init(v T)
        self.v = v

    public fn pick(other T) T
        return smaller(self.v, other)

fn main()
    let box = Box<P>(P(1))
    let r = box.pick(P(2))
    println(f"{r.v}")
"#,
        "'pick' orders its 'T' parameter",
    );
}

#[test]
fn test_a_body_that_calls_itself_at_a_deeper_type_is_refused() {
    // Each level instantiates `nest` at a type one list deeper than its own, so
    // the program needs `List` nested without end. Following instantiations
    // transitively stops at the depth one instance may nest to, and the program
    // is refused there rather than run the levels past it on a body shared by
    // every type.
    assert_build_error(
        r#"
use system.collections.list

fn nest<T>(a T, n int) int
    if n == 0
        return 0
    var l = List<T>()
    l.push(a)
    return 1 + nest(l, n - 1)

fn main()
    println(f"{nest(1, 3)}")
"#,
        "nests its type argument 33 levels deep",
    );
}
