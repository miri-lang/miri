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

/// A borrow's range runs to its last read: a binding read only before the
/// method replacing the matched value borrows from it.
#[test]
fn a_payload_binding_read_only_before_the_matched_value_is_replaced_borrows() {
    let source = format!(
        "{PACK}
fn main()
    var p = Pack.Two(Item(n: 1), Item(n: 2))
    match p
        Pack.Two(a, _)
            let _k = a.n
            p.clear()
        Pack.Empty: println(\"none\")
"
    );
    let (owners, body) = owners_of(&source, "main", "a");
    assert!(
        owners.iter().any(|owner| owner == "p"),
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

/// A function that only reads the matched value it is lent leaves the
/// payload a binding reads out of it alive.
#[test]
fn a_payload_binding_borrows_across_a_function_reading_the_matched_value() {
    let source = matching_pack("let _n = count(p)")
        + "
fn count(p Pack) int
    return p.size()
";
    let (owners, body) = owners_of(&source, "main", "a");
    assert!(
        owners.iter().any(|owner| owner == "p"),
        "{owners:?}\n{body}"
    );
}

/// A function replacing the matched value it is lent, through a method that
/// replaces its receiver, releases the payload a binding reads out of it, so
/// the binding cannot borrow from the matched value.
#[test]
fn a_function_replacing_the_matched_value_ends_the_borrow() {
    let source = matching_pack("wipe(p)")
        + "
fn wipe(p Pack)
    p.clear()
";
    let (owners, body) = owners_of(&source, "main", "a");
    assert!(
        !owners.iter().any(|owner| owner == "p"),
        "{owners:?}\n{body}"
    );
}

/// `main`, matching the `pack` field of a holder and running `store` in the
/// arm before reading a field of the payload binding `a`.
fn matching_held_pack(store: &str) -> String {
    format!(
        "{PACK}
class Holder
    public var pack Pack
    public var count int

fn main()
    var h = Holder(pack: Pack.Two(Item(n: 1), Item(n: 2)), count: 0)
    match h.pack
        Pack.Two(a, _)
            {store}
            let _k = a.n
        Pack.Empty: println(\"none\")
"
    )
}

/// A store to a field beside the matched one leaves the payload a binding
/// reads out of it where it was, so the binding borrows from the holder.
#[test]
fn a_payload_binding_borrows_across_a_store_beside_the_matched_field() {
    let (owners, body) = owners_of(&matching_held_pack("h.count = 1"), "main", "a");
    assert!(
        owners.iter().any(|owner| owner == "h"),
        "{owners:?}\n{body}"
    );
}

/// A store to the matched field itself releases the payload a binding reads
/// out of it, so the binding cannot borrow from the holder.
#[test]
fn a_store_to_the_matched_field_ends_the_borrow() {
    let (owners, body) = owners_of(&matching_held_pack("h.pack = Pack.Empty"), "main", "a");
    assert!(
        !owners.iter().any(|owner| owner == "h"),
        "{owners:?}\n{body}"
    );
}
