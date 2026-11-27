// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An assignment is an expression whose value is its target's, so `let y =
//! (x = 3)` binds 3. When the target's type holds a resource the store keeps
//! it, and the assignment's value would be a second holder of it, its hook
//! running when that holder lets go rather than when the target does. So
//! reading the value of an assignment that stores a resource is refused
//! (MER_OWN_012) — as an initializer, an argument, a returned value or the
//! right side of a chained assignment — with the repair: write the assignment
//! as a statement of its own, then read the target. The assignment as a
//! statement, and the value of one that stores a plain value, are accepted.

use super::super::utils::*;

const CODE: &str = "MER_OWN_012";

const RES: &str = r#"
use system.collections.list

class Res
    var id int
    fn drop(self)
        println(f"drop {self.id}")

class Holder
    var r Res
"#;

fn with_res(program: &str) -> String {
    format!("{RES}{program}")
}

/// Asserts `program` is refused at the line marked `READ`, with a message
/// naming the assignment's target `target` and a repair that writes the
/// assignment on its own and then reads the target.
fn assert_refused_read(program: &str, target: &str) {
    let diagnostic = assert_refused_at(&with_res(program), CODE, "READ");
    let spelled = format!("`{target}`");
    assert!(
        diagnostic.message.contains(&spelled) && diagnostic.message.contains("one holder"),
        "the refusal names the target {spelled} and the rule: {diagnostic:#?}"
    );
    let help = diagnostic.help.unwrap_or_default();
    assert!(
        help.contains("statement of its own") && help.contains(&format!("then read {spelled}")),
        "the refusal gives the repair: {help}"
    );
}

#[test]
fn reading_an_element_store_of_a_resource_is_refused() {
    assert_refused_read(
        r#"
fn main()
    var l = List([Res(9)])
    let a = Res(1)
    let y = (l[0] = a) // READ
    println(f"y {y.id}")
"#,
        "l[0]",
    );
}

#[test]
fn reading_a_variable_store_of_a_resource_is_refused() {
    assert_refused_read(
        r#"
fn main()
    var x = Res(9)
    let a = Res(1)
    let y = (x = a) // READ
    println(f"y {y.id}")
"#,
        "x",
    );
}

#[test]
fn reading_a_field_store_of_a_resource_is_refused() {
    assert_refused_read(
        r#"
fn main()
    var h = Holder(r: Res(9))
    let y = (h.r = Res(1)) // READ
    println(f"y {y.id}")
"#,
        "h.r",
    );
}

#[test]
fn reading_a_store_of_a_matched_payload_is_refused() {
    assert_refused_read(
        r#"
fn main()
    var l = List([Res(9)])
    let o Res? = Res(1)
    match o
        Some(r)
            let y = (l[0] = r) // READ
            println(f"y {y.id}")
        None: println("none")
"#,
        "l[0]",
    );
}

#[test]
fn passing_a_resource_store_as_an_argument_is_refused() {
    assert_refused_read(
        r#"
fn show(r Res)
    println(f"show {r.id}")

fn main()
    var x = Res(9)
    show(x = Res(1)) // READ
"#,
        "x",
    );
}

#[test]
fn chaining_a_resource_store_is_refused() {
    assert_refused_read(
        r#"
fn main()
    var p = Res(8)
    var q = Res(9)
    p = q = Res(1) // READ
    println(f"p {p.id}")
"#,
        "q",
    );
}

#[test]
fn returning_a_resource_store_is_refused() {
    assert_refused_read(
        r#"
fn swap_in(l out List<Res>, r Res) Res
    return (l[0] = r) // READ

fn main()
    var l = List([Res(9)])
    let y = swap_in(l, Res(1))
    println(f"y {y.id}")
"#,
        "l[0]",
    );
}

#[test]
fn reading_a_store_of_a_tuple_holding_a_resource_is_refused() {
    assert_refused_read(
        r#"
fn main()
    var t = (Res(9), 0)
    let y = (t = (Res(1), 1)) // READ
    println(f"y {y.1}")
"#,
        "t",
    );
}

#[test]
fn an_element_store_then_a_read_of_the_element_has_one_holder() {
    assert_runs_with_output(
        &with_res(
            r#"
fn main()
    var l = List([Res(9)])
    let a = Res(1)
    l[0] = a
    let y = l[0]
    println(f"y {y.id}")
"#,
        ),
        "drop 9\ny 1\ndrop 1",
    );
}

#[test]
fn a_variable_store_then_a_read_of_the_variable_has_one_holder() {
    assert_runs_with_output(
        &with_res(
            r#"
fn main()
    var x = Res(9)
    let a = Res(1)
    x = a
    let y = x
    println(f"y {y.id}")
"#,
        ),
        "drop 9\ny 1\ndrop 1",
    );
}

#[test]
fn a_payload_store_then_a_read_of_the_element_has_one_holder() {
    assert_runs_with_output(
        &with_res(
            r#"
fn main()
    var l = List([Res(9)])
    let o Res? = Res(1)
    match o
        Some(r)
            l[0] = r
            let y = l[0]
            println(f"y {y.id}")
        None: println("none")
"#,
        ),
        "drop 9\ny 1\ndrop 1",
    );
}

#[test]
fn the_value_of_an_assignment_storing_a_plain_value_is_read() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var x = 0
    var l = List([0])
    let y = (x = 3)
    let z = (l[0] = 4)
    var p = 0
    p = x = 5
    println(f"{y} {z} {p} {x} {l[0]}")
"#,
        "3 4 5 5 4",
    );
}

#[test]
fn a_generic_body_returning_a_store_is_refused_where_it_stores_a_resource() {
    assert_refused_read(
        r#"
fn put<T>(l out List<T>, v T) T
    return (l[0] = v) // READ

fn main()
    var l = List([Res(9)])
    let y = put(l, Res(1))
    println(f"y {y.id}")
"#,
        "l[0]",
    );
}

#[test]
fn a_generic_body_returning_a_store_of_a_plain_value_runs() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn put<T>(l out List<T>, v T) T
    return (l[0] = v)

fn main()
    var l = List([1])
    let y = put(l, 3)
    println(f"{y} {l[0]}")
"#,
        "3 3",
    );
}

#[test]
fn a_store_standing_as_a_one_line_arm_or_branch_has_one_holder() {
    assert_runs_with_output(
        &with_res(
            r#"
fn main()
    var l = List([Res(9)])
    let o Res? = Res(1)
    match o
        Some(r): l[0] = r
        None: println("none")
    var x = Res(5)
    if l.length() > 0: x = Res(6)
    println(f"{l[0].id} {x.id}")
"#,
        ),
        "drop 9\ndrop 5\n1 6\ndrop 6\ndrop 1",
    );
}
