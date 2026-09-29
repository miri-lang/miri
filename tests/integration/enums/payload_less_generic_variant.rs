// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A variant carrying no payload gives nothing to infer the enum's type
//! arguments from, so it is typed as the bare enum — the enum with its
//! instantiation not yet known. Wherever the slot it is written into names an
//! instantiation, that is the answer; a variant carrying a payload infers the
//! same thing from the payload instead.
//!
//! The stdlib's `Option` never hit this because `None` is special-cased, which
//! is why a user-written enum with a `Nothing` was unusable in every annotated
//! position.

use super::utils::*;

const HOLDER: &str = r#"
enum Holder<T>
    Of(T)
    Nothing

    fn empty() bool
        match self
            Holder.Of(_): false
            Holder.Nothing: true
"#;

fn with_holder(main: &str) -> String {
    format!("{HOLDER}\n{main}")
}

#[test]
fn a_payload_less_variant_fills_an_annotated_binding() {
    assert_runs_with_output(
        &with_holder(
            r#"
fn main()
    let n Holder<int> = Holder.Nothing
    println(f"{n.empty()}")
"#,
        ),
        "true",
    );
}

#[test]
fn a_payload_less_variant_fills_an_argument() {
    assert_runs_with_output(
        &with_holder(
            r#"
fn takes(h Holder<int>) bool
    return h.empty()

fn main()
    println(f"{takes(Holder.Nothing)}")
"#,
        ),
        "true",
    );
}

#[test]
fn a_payload_less_variant_fills_a_return_slot() {
    assert_runs_with_output(
        &with_holder(
            r#"
fn none_of() Holder<int>
    return Holder.Nothing

fn main()
    println(f"{none_of().empty()}")
"#,
        ),
        "true",
    );
}

#[test]
fn a_payload_less_variant_fills_a_field() {
    assert_runs_with_output(
        &with_holder(
            r#"
class Box
    slot Holder<int>

    fn init()
        self.slot = Holder.Nothing

fn main()
    let b = Box()
    println(f"{b.slot.empty()}")
"#,
        ),
        "true",
    );
}

#[test]
fn a_payload_less_variant_fills_a_collection_element() {
    assert_runs_with_output(
        &with_holder(
            r#"
use system.collections.list

fn main()
    var xs = List<Holder<int>>()
    xs.push(Holder.Nothing)
    println(f"{xs.length()} {xs[0].empty()}")
"#,
        ),
        "1 true",
    );
}

/// A payload carries its own instantiation, so it is still checked against the
/// slot rather than taking it.
#[test]
fn a_payload_carrying_variant_at_a_different_instantiation_is_still_refused() {
    assert_compiler_error(
        &with_holder(
            r#"
fn main()
    let n Holder<int> = Holder.Of("x")
    println(f"{n.empty()}")
"#,
        ),
        "expected Holder<int>, got Holder<String>",
    );
}

/// With no slot naming an instantiation there is nothing to take, and picking
/// one arbitrarily would be worse than saying so.
#[test]
fn a_payload_less_variant_with_no_expected_type_stays_uninstantiated() {
    assert_runs_with_output(
        &with_holder(
            r#"
fn main()
    let n = Holder.Nothing
    println(f"{n.empty()}")
"#,
        ),
        "true",
    );
}

#[test]
fn a_non_generic_enum_is_unaffected() {
    assert_runs_with_output(
        r#"
enum Plain
    A
    B

    fn is_a() bool
        match self
            Plain.A: true
            Plain.B: false

fn main()
    let p Plain = Plain.A
    println(f"{p.is_a()}")
"#,
        "true",
    );
}

/// A payload-less variant of an enum whose other variant holds a managed
/// value takes the instantiation of every location it is written into, so the
/// enum values beside it are released at that instantiation too.
#[test]
fn a_payload_less_variant_beside_managed_payloads_is_released_at_its_slot() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

enum Slot<T>
    Full(T)
    Empty

class Box
    var slot Slot<String>

fn show(s Slot<String>) String
    match s
        Slot.Full(v): v
        Slot.Empty: "empty"

fn main()
    let a = "a"
    var xs = List<Slot<String>>()
    xs.push(Slot.Full(f"{a}1"))
    xs.push(Slot.Empty)
    var b = Box(slot: Slot.Full(f"{a}2"))
    b.slot = Slot.Empty
    println(f"{show(xs[0])} {show(xs[1])} {show(b.slot)} {show(Slot.Empty)}")
"#,
        "a1 empty empty empty",
    );
}

/// An error built with no annotation leaves the ok type unbound; the value
/// holds only the error, which is released.
#[test]
fn an_unannotated_err_releases_its_managed_payload() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let r = Result.Err("e" + "rr")
    match r
        Result.Ok(v): println("ok")
        Result.Err(e): println(e)
"#,
        "err",
    );
}
