// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for borrow inference over whole programs: a binding read out of a
//! value the body owns borrows from it while nothing changes that value, and
//! a call handed the value as its receiver changes it unless its callee only
//! reads its receiver.

use crate::mir::utils::find_local_idx;
use miri::mir::Local;
use miri::pipeline::Pipeline;

/// The names of the locals the binding `name` of the body `function` of
/// `source` borrows from, after every pass up to reference counting; with
/// the body, to show when a test fails.
fn owners_of(source: &str, function: &str, name: &str) -> (Vec<String>, String) {
    let (bodies, facts) = Pipeline::new()
        .get_mir_program_with_rc(source)
        .expect("the program compiles");
    let (body, facts) = bodies
        .iter()
        .zip(facts.iter())
        .find(|((symbol, _), _)| symbol == function || symbol.ends_with(&format!(".{function}")))
        .map(|((_, body), facts)| (body, facts))
        .unwrap_or_else(|| panic!("no body named {function}"));
    let local = find_local_idx(body, name).unwrap_or_else(|| panic!("no local {name}:\n{body}"));
    let owners = (facts.ownership().borrowed_locals.get(&Local(local)))
        .into_iter()
        .flatten()
        .filter_map(|owner| {
            body.local_decls[owner.0]
                .name
                .as_deref()
                .map(str::to_string)
        })
        .collect();
    (owners, body.to_string())
}

/// `Pack`, an enum with a drop hook whose `size` only reads its receiver and
/// whose `clear` replaces it. A match names a value of it rather than
/// copying it.
const PACK: &str = r#"
class Item
    public var n int

enum Pack
    Two(Item, Item)
    Empty

    fn drop(self)
        println("dropped")

    fn size(self) int
        match self
            Pack.Two(_, _): return 2
            Pack.Empty: return 0

    fn clear(self)
        self = Pack.Empty
"#;

/// `main`, matching `p` and running `call` in the arm before reading a
/// field of the payload binding `a`.
fn matching_pack(call: &str) -> String {
    format!(
        "{PACK}
fn main()
    var p = Pack.Two(Item(n: 1), Item(n: 2))
    match p
        Pack.Two(a, _)
            {call}
            let _k = a.n
        Pack.Empty: println(\"none\")
"
    )
}

/// A method that only reads its receiver leaves the payload a binding reads
/// out of it alive.
#[test]
fn a_payload_binding_borrows_across_a_method_reading_the_matched_value() {
    let (owners, body) = owners_of(&matching_pack("let _n = p.size()"), "main", "a");
    assert!(
        owners.iter().any(|owner| owner == "p"),
        "{owners:?}\n{body}"
    );
}

/// A method replacing its receiver releases the payload a binding reads out
/// of it, so the binding cannot borrow from the matched value.
#[test]
fn a_method_replacing_the_matched_value_ends_the_borrow() {
    let (owners, body) = owners_of(&matching_pack("p.clear()"), "main", "a");
    assert!(
        !owners.iter().any(|owner| owner == "p"),
        "{owners:?}\n{body}"
    );
}

/// The payload binding of a matched optional borrows from it across
/// `is_some`, which only reads its receiver.
#[test]
fn a_payload_binding_borrows_across_a_method_reading_the_optional() {
    let source = r#"
class Token
    public var id int
    fn drop(self)
        println(f"released {self.id}")

fn main()
    let p Token? = Some(Token(id: 3))
    match p
        Some(r)
            println(f"has {p.is_some()}")
            println(f"{r.id}")
        None: println("none")
"#;
    let (owners, body) = owners_of(source, "main", "r");
    assert!(
        owners.iter().any(|owner| owner == "p"),
        "{owners:?}\n{body}"
    );
}
