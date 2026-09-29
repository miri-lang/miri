// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A variant constructor binds only the type arguments its payload names:
//! `E.L(s)` is an `E<String, B>`, with `B` a slot nothing chose. A value of it
//! is laid out without `B`, so a value that binds `B` — an `E<String, i128>`,
//! whose payload slots are wider — cannot share a location with it. Where the
//! two meet in one expression (the branches of a conditional or a match, the
//! elements of a literal) the whole expression takes the type binding both,
//! and every part is built at it. Where a location already holds one and is
//! handed the other later (an assignment, a push), the program is refused and
//! asked to declare the type.

use super::super::utils::*;

const E: &str = r#"
use system.collections.list

enum E<A, B>
    L(A)
    R(B, A)

fn make() E<String, i128>
    return E.R(5, "t" + "u")

fn show(e E<String, i128>)
    match e
        E.R(n, s): println(f"r {s}")
        E.L(s): println(f"l {s}")
"#;

fn with_e(main: &str) -> String {
    format!("{E}\n{main}")
}

#[test]
fn reassigning_a_binding_a_value_that_binds_its_open_argument_is_refused() {
    assert_compiler_error(
        &with_e(
            r#"
fn main()
    var e = E.L("s" + "")
    e = make()
    show(e)
"#,
        ),
        "leaves `B` unbound",
    );
}

#[test]
fn reassigning_it_on_one_branch_only_is_refused() {
    assert_compiler_error(
        &with_e(
            r#"
fn pick(c bool)
    var e = E.L("s" + "")
    if c
        e = make()
    show(e)

fn main()
    pick(true)
"#,
        ),
        "leaves `B` unbound",
    );
}

#[test]
fn pushing_a_value_that_binds_the_elements_open_argument_is_refused() {
    assert_compiler_error(
        &with_e(
            r#"
fn main()
    let xs = List([E.L("s" + "")])
    xs.push(make())
    println(f"{xs.length()}")
"#,
        ),
        "leaves `B` unbound",
    );
}

#[test]
fn a_declared_binding_holds_either_variant() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn main()
    var e E<String, i128> = E.L("s" + "")
    show(e)
    e = make()
    show(e)
"#,
        ),
        "l s\nr tu",
    );
}

#[test]
fn a_conditional_joins_its_branches_whichever_is_taken() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn pick(c bool)
    let e = if c: E.L("s" + "") else: make()
    show(e)

fn main()
    pick(true)
    pick(false)
"#,
        ),
        "l s\nr tu",
    );
}

#[test]
fn a_match_joins_its_arms() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn pick(n int)
    let e = match n
        0: E.L("s" + "")
        _: make()
    show(e)

fn main()
    pick(0)
    pick(1)
"#,
        ),
        "l s\nr tu",
    );
}

#[test]
fn a_list_literal_joins_its_elements() {
    assert_heap_guard_output(
        &with_e(
            r#"
fn main()
    let xs = List([E.L("s" + ""), make()])
    for x in xs
        show(x)
"#,
        ),
        "l s\nr tu",
    );
}

#[test]
fn a_result_reassigned_a_value_binding_its_open_error_is_refused() {
    assert_compiler_error(
        r#"
fn other() Result<String, String>
    return Result.Err("e" + "f")

fn main()
    var r = Result.Ok("s" + "")
    match r
        Result.Ok(v): println(v)
        Result.Err(e): println(e)
    let _assigned = (r = other())
    match r
        Result.Ok(v): println(v)
        Result.Err(e): println(e)
"#,
        "leaves `E` unbound",
    );
}

#[test]
fn a_declared_result_is_reassigned_either_variant() {
    assert_heap_guard_output(
        r#"
fn other() Result<String, String>
    return Result.Err("e" + "f")

fn main()
    var r Result<String, String> = Result.Ok("s" + "")
    match r
        Result.Ok(v): println(v)
        Result.Err(e): println(e)
    let _assigned = (r = other())
    match r
        Result.Ok(v): println(v)
        Result.Err(e): println(e)
"#,
        "s\nef",
    );
}

#[test]
fn a_map_literal_joins_its_values() {
    assert_heap_guard_output(
        &with_e(
            r#"
use system.collections.map

fn main()
    let m = {1: E.L("s" + ""), 2: make()}
    show(m[1])
    show(m[2])
"#,
        ),
        "l s\nr tu",
    );
}

#[test]
fn a_map_literal_joins_its_keys() {
    assert_heap_guard_output(
        &with_e(
            r#"
use system.collections.map

fn main()
    let m = {Result.Err("e" + "1"): 1, Result.Ok("o" + "k"): 2}
    println(f"{m.length()}")
"#,
        ),
        "2",
    );
}

#[test]
fn a_set_literal_joins_its_elements() {
    assert_heap_guard_output(
        &with_e(
            r#"
use system.collections.set

fn main()
    let s = {Result.Err("e" + "1"), Result.Ok("o" + "k")}
    println(f"{s.length()}")
"#,
        ),
        "2",
    );
}
