// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_null_coalesce_some() {
    assert_runs_with_output(
        r#"

let x int? = 42
println(f"{x ?? 0}")
"#,
        "42",
    );
}

#[test]
fn test_null_coalesce_none() {
    assert_runs_with_output(
        r#"

let x int? = None
println(f"{x ?? 99}")
"#,
        "99",
    );
}

#[test]
fn test_null_coalesce_string_some() {
    assert_runs_with_output(
        r#"

let s String? = "hello"
println(s ?? "default")
"#,
        "hello",
    );
}

#[test]
fn test_null_coalesce_string_none() {
    assert_runs_with_output(
        r#"

let s String? = None
println(s ?? "default")
"#,
        "default",
    );
}

#[test]
fn test_null_coalesce_with_function_call() {
    assert_runs_with_output(
        r#"

fn f() int?
    return None

println(f"{f() ?? 0}")
"#,
        "0",
    );
}

#[test]
fn test_null_coalesce_with_some_constructor() {
    assert_runs_with_output(
        r#"

let x = Some(42)
println(f"{x ?? 0}")
"#,
        "42",
    );
}

const COALESCED_BOX: &str = r#"
class Box
    var v int
    fn init(v int)
        self.v = v
"#;

/// A fallback that is a binding is read, not moved: the binding still holds
/// its value after the operator, on the path that took the fallback and on
/// the one that did not, and the value is released once.
#[test]
fn a_coalesce_fallback_binding_is_shared_not_moved() {
    let code = format!(
        "{COALESCED_BOX}
fn pick(o Box?, b Box) Box
    return o ?? b

fn main()
    let b = Box(1)
    let none Box? = None
    let some Box? = Box(2)
    var c = none ?? b
    c.v = 7
    let d = some ?? b
    let e = pick(None, b)
    println(f\"{{b.v}} {{c.v}} {{d.v}} {{e.v}} {{(none ?? b).v}}\")
"
    );
    assert_heap_guard_output(&code, "7 7 2 7 7");
}

#[test]
fn a_coalesce_fallback_string_binding_is_shared_not_moved() {
    assert_heap_guard_output(
        r#"
fn main()
    let fallback = "fall" + "back"
    let none String? = None
    let some String? = "some" + "thing"
    let a = none ?? fallback
    let b = some ?? fallback
    println(f"{a} {b} {fallback}")
"#,
        "fallback something fallback",
    );
}
