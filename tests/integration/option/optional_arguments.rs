// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A value passed where its optional is declared is wrapped on the way, so
//! the callee reads a `Some` — whether the parameter is written `int?`, is a
//! type parameter a call binds to `int?`, or belongs to a method. A bare value
//! read as the address of an optional is a crash, or garbage.

use super::super::utils::*;

#[test]
fn a_plain_value_passed_to_an_optional_method_parameter_is_wrapped() {
    assert_heap_guard_output(
        r#"
use system.io

class C
    k int
    fn put(x int?) int?
        return x

fn main()
    let c = C(k: 1)
    let r = c.put(7)
    match r
        Some(n): println(f"{n}")
        None: println("none")
"#,
        "7",
    );
}

#[test]
fn a_plain_value_passed_where_a_call_binds_t_to_an_optional_is_wrapped() {
    assert_heap_guard_output(
        r#"
use system.io

fn show<T>(a T, b T) T
    return b

fn main()
    let o int? = 6
    let r = show(o, 7)
    match r
        Some(n): println(f"{n}")
        None: println("none")
"#,
        "7",
    );
}

#[test]
fn a_plain_value_passed_to_a_generic_method_at_an_optional_instance_is_wrapped() {
    assert_heap_guard_output(
        r#"
use system.io

class Box<T>
    v T
    fn put(x T) T
        self.v = x
        return self.v

fn main()
    var b = Box<int?>(v: None)
    let r = b.put(7)
    match r
        Some(n): println(f"{n}")
        None: println("none")
"#,
        "7",
    );
}

#[test]
fn a_plain_value_passed_to_an_optional_parameter_through_a_trait_is_wrapped() {
    assert_heap_guard_output(
        r#"
use system.io

trait Tr
    fn put(x int?) int

class C implements Tr
    k int
    fn put(x int?) int
        let v = x ?? -1
        return v

fn main()
    let t Tr = C(k: 1)
    let a = t.put(7)
    let b = t.put(None)
    println(f"{a} {b}")
"#,
        "7 -1",
    );
}

/// A managed value is retained once for the optional that wraps it, whether
/// the caller reads its own binding again or not.
#[test]
fn a_managed_value_wrapped_for_an_optional_parameter_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.io

fn put(x String?) String
    return x ?? "none"

class C
    k int
    fn put(x String?) String
        return x ?? "none"

fn main()
    let s = "a" + "b"
    println(put(s))
    println(s)
    let c = C(k: 1)
    let t = "c" + "d"
    println(c.put(t))
    println(c.put("e" + "f"))
"#,
        "ab\nab\ncd\nef",
    );
}

#[test]
fn assert_eq_between_an_optional_and_a_plain_value_compares_them_as_optionals() {
    assert_runs_with_output(
        r#"
use system.io
use system.testing

fn main()
    let o int? = 6
    assert_eq(o, 6)
    let xs = [1, 2, 3]
    assert_eq(xs.sum(), 6)
    println("ok")
"#,
        "ok",
    );
}

#[test]
fn assert_eq_between_an_optional_and_a_different_plain_value_fails() {
    assert_runtime_error(
        r#"
use system.testing

fn main()
    let o int? = 6
    assert_eq(o, 7)
"#,
        "expected Some(7), got Some(6)",
    );
}

/// A binding declared with the written `Option<int>` already holds an
/// optional, so it is passed as it is rather than wrapped a second time.
#[test]
fn an_optional_declared_as_option_of_t_is_not_wrapped_again() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn count(ea Option<int>, eb Option<int>) int
    var l = List<Option<int>>()
    l.push(Some(4))
    var r = 0
    if l.contains(ea)
        r = r + 1
    if l.contains(eb)
        r = r + 2
    return r

fn main()
    let a Option<int> = Some(4)
    println(f"{count(a, None)}")
"#,
        "1",
    );
}
