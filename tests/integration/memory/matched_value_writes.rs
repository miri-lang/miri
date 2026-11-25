// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A write to the value a `match` reads ends the bindings its arm's patterns
//! read out of it. Assigning the matched place or a place containing it,
//! passing either `out`, or calling a method that writes either, releases
//! the payload a binding reads, so a later use of a binding that reaches a
//! drop hook is refused (MER_OWN_011), with the repair: use the binding
//! before the write, or write after the match. A binding read only before the
//! write keeps borrowing the payload to its last use, so the hook check sees
//! one holder; a write beside the matched place, and a payload that reaches
//! no hook, are accepted.

use super::super::utils::*;
use crate::utils::miri_run_with_env;

const CODE: &str = "MER_OWN_011";

const PACK: &str = r#"
class Token
    public var id int
    fn drop(self)
        println(f"released {self.id}")

enum Pack
    Two(Token, Token)
    Empty
    fn drop(self)
        println("pack dropped")
    fn clear(self)
        self = Pack.Empty

class Holder
    public var pack Pack
    public var count int
    fn reset(self)
        self.pack = Pack.Empty

fn empty(p out Pack)
    p = Pack.Empty
"#;

fn with_pack(program: &str) -> String {
    format!("{PACK}{program}")
}

/// Run `code` with the hook check on, expecting `expected` and a clean exit.
fn assert_one_holder_output(code: &str, expected: &str) {
    let result = miri_run_with_env(code, "MIRI_HOOK_CHECK", "1");
    assert!(
        result.success,
        "expected a clean run under MIRI_HOOK_CHECK=1:\n{}",
        result.output()
    );
    assert_eq!(result.stdout.trim_end(), expected, "{}", result.stderr);
}

/// Asserts `program` is refused at the use of `a` after a write ended it,
/// with a message naming the write and a repair.
fn assert_refused_use(program: &str, write: &str) {
    let code = with_pack(program);
    let diagnostic = assert_refused_at(&code, CODE, "USE");
    assert!(
        diagnostic.message.contains("`a`") && diagnostic.message.contains(write),
        "the refusal names the binding and the write '{write}': {diagnostic:#?}"
    );
    let help = diagnostic.help.unwrap_or_default();
    assert!(
        help.contains("before") && help.contains("after the match"),
        "the refusal gives the repair: {help}"
    );
}

#[test]
fn a_payload_used_after_a_method_replaces_the_matched_value_is_refused() {
    assert_refused_use(
        r#"
fn main()
    var p = Pack.Two(Token(id: 1), Token(id: 2))
    match p
        Pack.Two(a, _)
            p.clear()
            println(f"{a.id}") // USE
        Pack.Empty: println("none")
"#,
        "p.clear()",
    );
}

#[test]
fn a_payload_used_after_the_matched_value_is_assigned_is_refused() {
    assert_refused_use(
        r#"
fn main()
    var p = Pack.Two(Token(id: 1), Token(id: 2))
    match p
        Pack.Two(a, _)
            p = Pack.Empty
            println(f"{a.id}") // USE
        Pack.Empty: println("none")
"#,
        "p",
    );
}

#[test]
fn a_payload_used_after_the_matched_value_is_passed_out_is_refused() {
    assert_refused_use(
        r#"
fn main()
    var p = Pack.Two(Token(id: 1), Token(id: 2))
    match p
        Pack.Two(a, _)
            empty(p)
            println(f"{a.id}") // USE
        Pack.Empty: println("none")
"#,
        "empty(p)",
    );
}

#[test]
fn a_payload_used_after_a_place_containing_the_matched_value_is_written_is_refused() {
    assert_refused_use(
        r#"
fn main()
    var h = Holder(pack: Pack.Two(Token(id: 1), Token(id: 2)), count: 0)
    match h.pack
        Pack.Two(a, _)
            h.reset()
            println(f"{a.id}") // USE
        Pack.Empty: println("none")
"#,
        "h.reset()",
    );
}

#[test]
fn a_payload_read_again_on_the_next_pass_of_a_loop_that_writes_the_matched_value_is_refused() {
    assert_refused_use(
        r#"
fn main()
    var p = Pack.Two(Token(id: 1), Token(id: 2))
    match p
        Pack.Two(a, _)
            var i = 0
            while i < 2
                println(f"{a.id}") // USE
                if i == 1: p.clear()
                i = i + 1
        Pack.Empty: println("none")
"#,
        "p.clear()",
    );
}

#[test]
fn a_payload_handed_to_the_call_that_writes_the_matched_value_is_refused() {
    let code = with_pack(
        r#"
fn refill(p out Pack, t Token)
    println(f"refill {t.id}")
    p = Pack.Empty

fn main()
    var p = Pack.Two(Token(id: 1), Token(id: 2))
    match p
        Pack.Two(a, _)
            refill(p, a) // USE
        Pack.Empty: println("none")
"#,
    );
    let diagnostic = assert_refused_at(&code, CODE, "USE");
    assert!(
        diagnostic.message.contains("handed to `refill(p)`"),
        "the refusal names the call writing the matched value: {diagnostic:#?}"
    );
}

#[test]
fn a_payload_of_a_value_matched_inside_an_arm_is_ended_by_a_write_to_the_outer_value() {
    let code = with_pack(
        r#"
fn main()
    var o Pack? = Some(Pack.Two(Token(id: 1), Token(id: 2)))
    match o
        Some(inner)
            match inner
                Pack.Two(a, _)
                    o = None
                    println(f"{a.id}") // USE
                Pack.Empty: println("empty")
        None: println("none")
"#,
    );
    let diagnostic = assert_refused_at(&code, CODE, "USE");
    let help = diagnostic.help.unwrap_or_default();
    assert!(
        help.contains("write `o` after the match"),
        "the repair names the place written: {help}"
    );
}

#[test]
fn a_payload_used_in_one_arm_while_another_arm_writes_the_matched_value_has_one_holder() {
    assert_one_holder_output(
        &with_pack(
            r#"
fn main()
    var p = Pack.Two(Token(id: 1), Token(id: 2))
    match p
        Pack.Two(a, _): println(f"{a.id}")
        Pack.Empty: p.clear()
    println("end")
"#,
        ),
        "1\nend\npack dropped\nreleased 1\nreleased 2",
    );
}

#[test]
fn a_payload_used_before_the_matched_value_is_replaced_has_one_holder() {
    assert_one_holder_output(
        &with_pack(
            r#"
fn main()
    var p = Pack.Two(Token(id: 1), Token(id: 2))
    match p
        Pack.Two(a, _)
            println(f"{a.id}")
            p.clear()
        Pack.Empty: println("none")
    println("end")
"#,
        ),
        "1\npack dropped\nreleased 1\nreleased 2\nend\npack dropped",
    );
}

#[test]
fn a_payload_used_in_its_arm_and_the_value_replaced_after_the_match_has_one_holder() {
    assert_one_holder_output(
        &with_pack(
            r#"
fn main()
    var p = Pack.Two(Token(id: 1), Token(id: 2))
    match p
        Pack.Two(a, _): println(f"{a.id}")
        Pack.Empty: println("none")
    p.clear()
    println("end")
"#,
        ),
        "1\npack dropped\nreleased 1\nreleased 2\nend\npack dropped",
    );
}

#[test]
fn a_payload_used_after_a_write_beside_the_matched_place_has_one_holder() {
    // The matched enum has no hook of its own here: with one, borrow
    // inference still takes a store to a sibling field for a change to the
    // subject's root (see `local_borrows::statement_changes`).
    assert_one_holder_output(
        &with_pack(
            r#"
enum Bag
    Two(Token, Token)
    Empty

class Shelf
    public var bag Bag
    public var count int

fn main()
    var h = Shelf(bag: Bag.Two(Token(id: 1), Token(id: 2)), count: 0)
    match h.bag
        Bag.Two(a, _)
            h.count = 1
            println(f"{a.id} {h.count}")
        Bag.Empty: println("none")
"#,
        ),
        "1 1\nreleased 1\nreleased 2",
    );
}

#[test]
fn a_payload_used_after_a_write_to_a_new_binding_shadowing_the_matched_name_has_one_holder() {
    assert_one_holder_output(
        &with_pack(
            r#"
fn main()
    var p = Pack.Two(Token(id: 1), Token(id: 2))
    match p
        Pack.Two(a, _)
            var p = Pack.Empty
            p.clear()
            println(f"{a.id}")
        Pack.Empty: println("none")
    println("end")
"#,
        ),
        "pack dropped\n1\npack dropped\nend\npack dropped\nreleased 1\nreleased 2",
    );
}

#[test]
fn a_payload_without_a_hook_used_after_the_matched_value_is_replaced_keeps_the_old_value() {
    assert_one_holder_output(
        r#"
class Box
    public var id int

enum Crate
    Two(Box, Box)
    Empty
    fn clear(self)
        self = Crate.Empty
    fn size(self) int
        match self
            Crate.Two(_, _): return 2
            Crate.Empty: return 0

fn main()
    var c = Crate.Two(Box(id: 1), Box(id: 2))
    match c
        Crate.Two(a, _)
            c.clear()
            println(f"{a.id} {c.size()}")
        Crate.Empty: println("none")
"#,
        "1 0",
    );
}
