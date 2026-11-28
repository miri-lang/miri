// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A function that binds its resource parameter to a local owns it, as one
//! storing it in a field or a collection does: the caller hands its
//! reference over at the call, the local is the one holder, and the hook
//! runs where the local ends. A later read of the argument in the caller is
//! refused. A function that only reads its parameter still lends it.

use super::super::utils::*;

const TOKEN: &str = r#"
use system.io

class Token
    public var id int
    fn drop(self)
        println(f"released {self.id}")
"#;

fn with_token(program: &str) -> String {
    format!("{TOKEN}{program}")
}

#[test]
fn a_parameter_bound_by_let_is_released_where_the_binding_ends() {
    assert_runs_with_output(
        &with_token(
            r#"
fn pick(t Token) int
    let b = t
    return b.id

fn main()
    let t = Token(id: 4)
    println(f"{pick(t)}")
    println("after")
"#,
        ),
        "released 4\n4\nafter",
    );
}

#[test]
fn a_parameter_bound_by_var_is_released_where_the_binding_ends() {
    assert_runs_with_output(
        &with_token(
            r#"
fn pick(t Token) int
    var b = t
    b.id = b.id + 1
    return b.id

fn main()
    println(f"{pick(Token(id: 4))}")
    println("after")
"#,
        ),
        "released 5\n5\nafter",
    );
}

#[test]
fn a_payload_of_a_parameter_bound_by_let_is_released_where_the_binding_ends() {
    assert_runs_with_output(
        &with_token(
            r#"
fn pick(p Token?) int
    match p
        Some(r)
            let b = r
            return b.id
        None: return 0

fn main()
    let p Token? = Some(Token(id: 6))
    println(f"{pick(p)}")
    println("after")
"#,
        ),
        "released 6\n6\nafter",
    );
}

#[test]
fn a_parameter_bound_on_one_path_is_released_once_on_each_path() {
    assert_runs_with_output(
        &with_token(
            r#"
fn pick(t Token, bind bool) int
    if bind
        let b = t
        println(f"bound {b.id}")
    println("end of pick")
    return 0

fn main()
    let _a = pick(Token(id: 1), true)
    let _b = pick(Token(id: 2), false)
    println("after")
"#,
        ),
        "bound 1\nreleased 1\nend of pick\nend of pick\nreleased 2\nafter",
    );
}

#[test]
fn a_parameter_assigned_to_a_local_is_released_where_the_local_lets_it_go() {
    assert_runs_with_output(
        &with_token(
            r#"
fn pick(t Token, u Token) int
    var b = t
    println(f"first {b.id}")
    b = u
    println(f"second {b.id}")
    return b.id

fn main()
    println(f"{pick(Token(id: 1), Token(id: 2))}")
    println("after")
"#,
        ),
        "first 1\nreleased 1\nsecond 2\nreleased 2\n2\nafter",
    );
}

#[test]
fn reading_an_argument_after_a_call_binding_it_through_a_conditional_is_refused() {
    assert_refused_at(
        &with_token(
            r#"
fn pick(t Token, c bool) int
    let b = t if c else Token(id: 0)
    return b.id

fn main()
    let t = Token(id: 1)
    println(f"{pick(t, true)}")
    println(f"{t.id}")
"#,
        ),
        "MER_OWN_003",
        "println(f\"{t.id}\")",
    );
}

#[test]
fn reading_an_optional_after_a_call_binding_its_payload_through_a_match_is_refused() {
    assert_refused_at(
        &with_token(
            r#"
fn pick(p Token?) int
    let b = match p
        Some(r): r
        None: Token(id: 0)
    return b.id

fn main()
    let p Token? = Some(Token(id: 2))
    println(f"{pick(p)}")
    println(f"{p.is_some()}")
"#,
        ),
        "MER_OWN_003",
        "println(f\"{p.is_some()}\")",
    );
}

#[test]
fn a_field_read_into_a_local_leaves_the_parameter_the_callers() {
    assert_runs_with_output(
        &with_token(
            r#"
class Named
    public var name String
    fn drop(self)
        println(f"released {self.name}")

fn label(t Named) int
    let s = t.name
    println(s)
    return s.length()

fn main()
    let t = Named(name: "a")
    println(f"{label(t)}")
    println(f"still {t.name}")
"#,
        ),
        "a\n1\nstill a\nreleased a",
    );
}

#[test]
fn a_drop_hook_binding_its_receiver_is_refused_at_the_binding() {
    assert_refused_at(
        &with_token(
            r#"
class Box
    public var id int
    fn drop(self)
        let me = self
        println(f"box {me.id}")

fn main()
    let _b = Box(id: 3)
"#,
        ),
        "MER_OWN_007",
        "let me = self",
    );
}

#[test]
fn a_parameter_only_read_stays_the_callers() {
    assert_runs_with_output(
        &with_token(
            r#"
fn read(t Token) int
    return t.id

fn main()
    let t = Token(id: 7)
    println(f"{read(t)}")
    println(f"still {t.id}")
"#,
        ),
        "7\nstill 7\nreleased 7",
    );
}

#[test]
fn reading_an_argument_after_a_call_binding_it_is_refused() {
    assert_refused_at(
        &with_token(
            r#"
fn pick(t Token) int
    let b = t
    return b.id

fn main()
    let t = Token(id: 4)
    println(f"{pick(t)}")
    println(f"{t.id}")
"#,
        ),
        "MER_OWN_003",
        "println(f\"{t.id}\")",
    );
}

#[test]
fn reading_an_optional_after_a_call_binding_its_payload_is_refused() {
    assert_refused_at(
        &with_token(
            r#"
fn pick(p Token?) int
    match p
        Some(r)
            let b = r
            return b.id
        None: return 0

fn main()
    let p Token? = Some(Token(id: 6))
    println(f"{pick(p)}")
    println(f"{p.is_some()}")
"#,
        ),
        "MER_OWN_003",
        "println(f\"{p.is_some()}\")",
    );
}
