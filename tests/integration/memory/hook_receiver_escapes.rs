// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A drop hook may not let `self` escape. While the hook runs, the
//! instance's count cannot reach zero, so whatever kept `self` past the hook
//! would hold an instance freed as soon as the hook returns. Every way the
//! escape analysis finds a hook body keeping its receiver is refused with
//! MER_OWN_007, at the line that keeps it, with a repair; a hook that only
//! reads `self` and lends it still runs.

use super::super::utils::*;
use crate::utils::miri_check_json;

const CODE: &str = "MER_OWN_007";

const PRELUDE: &str = r#"use system.collections.list

struct Shelf
    slot Tag?

class Holder
    public var slot Tag?
    fn init()
        self.slot = None

enum Slot
    Empty
    Full(Tag)

trait Sink
    fn take(t Tag) int

class Keeper implements Sink
    fn init()
        return
    public fn take(t Tag) int
        let _kept = Shelf(slot: Some(t))
        return 0

class Looker implements Sink
    fn init()
        return
    public fn take(t Tag) int
        return t.id

fn keep(t Tag) int
    let _held = Shelf(slot: Some(t))
    return 0

fn relay(t Tag) int
    return keep(t)

fn look(t Tag) int
    return t.id

class Inner
    public var n int
    fn init(n int)
        self.n = n
    fn drop(self)
        println(f"inner {self.n}")

fn peek(i Inner) int
    return i.n

class Tag
    public var id int
    public var inner Inner
    fn init(id int)
        self.id = id
        self.inner = Inner(id)
    fn describe() int
        return self.id
    fn drop(self)
"#;

const MAIN: &str = r#"
fn main()
    let a = Tag(1)
    println(f"{a.id}")
"#;

/// The program whose `Tag` hook runs `body`, each line indented into it.
fn hook_running(body: &[&str]) -> String {
    let lines: String = (body.iter())
        .map(|line| format!("        {line}\n"))
        .collect();
    format!("{PRELUDE}{lines}{MAIN}")
}

/// The 1-based line of the first line of `source` containing `marker`.
fn line_of(source: &str, marker: &str) -> usize {
    (source.lines().position(|line| line.contains(marker)))
        .map(|index| index + 1)
        .unwrap_or_else(|| panic!("'{marker}' is in the program:\n{source}"))
}

/// Asserts the hook running `body` is refused with MER_OWN_007 at the line
/// containing `escaping`, the refusal carrying the repair.
fn assert_escape_refused(body: &[&str], escaping: &str) {
    let source = hook_running(body);
    assert_escape_refused_in(&source, escaping);
}

/// [`assert_escape_refused`] for a whole program.
fn assert_escape_refused_in(source: &str, escaping: &str) {
    let envelope = miri_check_json(source);
    let diagnostic = (envelope.diagnostics.iter())
        .find(|d| d.code.as_deref() == Some(CODE))
        .unwrap_or_else(|| panic!("a {CODE} refusal: {envelope:#?}"));
    assert_eq!(
        diagnostic.line,
        Some(line_of(source, escaping)),
        "the refusal points at '{escaping}': {diagnostic:#?}"
    );
    assert!(
        diagnostic.message.contains("'self'"),
        "the refusal names self: {diagnostic:#?}"
    );
    let help = diagnostic.help.as_deref().unwrap_or_default();
    assert!(
        help.contains("copy the plain data"),
        "the refusal carries the repair: {diagnostic:#?}"
    );
}

/// Asserts the refusal of the hook running `body` names the hop `hop` at
/// the line containing `marker`.
fn assert_hop_at(body: &[&str], hop: &str, marker: &str) {
    let source = hook_running(body);
    let envelope = miri_check_json(&source);
    let diagnostic = (envelope.diagnostics.iter())
        .find(|d| d.code.as_deref() == Some(CODE))
        .unwrap_or_else(|| panic!("a {CODE} refusal: {envelope:#?}"));
    let line = line_of(&source, marker);
    assert!(
        (diagnostic.related.iter())
            .any(|related| related.message.contains(hop) && related.line == Some(line)),
        "a related entry names '{hop}' at line {line}: {diagnostic:#?}"
    );
}

#[test]
fn a_hook_building_a_struct_holding_self_is_refused() {
    assert_escape_refused(
        &["let _s = Shelf(slot: Some(self))"],
        "Shelf(slot: Some(self))",
    );
}

#[test]
fn a_hook_storing_self_into_another_objects_field_is_refused() {
    assert_escape_refused(
        &[
            "var h = Holder()",
            "h.slot = Some(self)",
            "println(f\"{h.slot == None}\")",
        ],
        "h.slot = Some(self)",
    );
}

#[test]
fn a_hook_pushing_self_into_a_list_is_refused() {
    assert_escape_refused(&["var l = List<Tag>()", "l.push(self)"], "l.push(self)");
}

#[test]
fn a_hook_building_a_list_literal_of_self_is_refused() {
    assert_escape_refused(&["let _l = [self]"], "let _l = [self]");
}

#[test]
fn a_hook_building_a_tuple_holding_self_is_refused() {
    assert_escape_refused(&["let _t = (self, 1)"], "let _t = (self, 1)");
}

#[test]
fn a_hook_building_a_variant_holding_self_is_refused() {
    assert_escape_refused(&["let _e = Slot.Full(self)"], "Slot.Full(self)");
}

#[test]
fn a_hook_wrapping_self_in_an_optional_is_refused() {
    assert_escape_refused(&["let _o = Some(self)"], "let _o = Some(self)");
}

#[test]
fn a_hook_capturing_self_in_a_closure_is_refused() {
    assert_escape_refused(
        &[
            "let f = fn() int",
            "    return self.id",
            "println(f\"{f()}\")",
        ],
        "let f = fn() int",
    );
}

#[test]
fn a_hook_handing_self_to_a_keeping_function_is_refused() {
    assert_escape_refused(&["keep(self)"], "        keep(self)");
}

/// The refusal follows the keep into the callee and names the line there
/// that keeps the instance.
#[test]
fn a_hook_handing_self_to_a_keeper_names_the_keepers_line() {
    assert_hop_at(
        &["keep(self)"],
        "builds a value",
        "let _held = Shelf(slot: Some(t))",
    );
}

#[test]
fn a_hook_handing_self_through_a_chain_of_calls_is_refused() {
    assert_escape_refused(&["relay(self)"], "        relay(self)");
    assert_hop_at(&["relay(self)"], "calls keep", "    return keep(t)");
}

#[test]
fn a_hook_handing_self_to_a_keeping_method_of_another_class_is_refused() {
    assert_escape_refused(&["Keeper().take(self)"], "Keeper().take(self)");
}

#[test]
fn a_hook_handing_self_to_a_trait_method_a_keeper_fills_is_refused() {
    assert_escape_refused(&["let k Sink = Keeper()", "k.take(self)"], "k.take(self)");
}

#[test]
fn a_hook_handing_self_to_a_keeper_held_in_a_local_is_refused() {
    assert_escape_refused(&["let f = keep", "f(self)"], "        f(self)");
}

#[test]
fn a_hook_handing_self_to_a_keeping_lambda_held_in_a_local_is_refused() {
    assert_escape_refused(
        &["let f = fn(x Tag) int", "    return keep(x)", "f(self)"],
        "        f(self)",
    );
}

#[test]
fn a_hook_handing_self_through_super_to_a_keeper_is_refused() {
    let source = r#"
struct Shelf
    slot Base?

class Base
    public var id int
    fn init(id int)
        self.id = id
    fn put_on() int
        let _shelf = Shelf(slot: Some(self))
        return 0

class Derived extends Base
    fn init(id int)
        super.init(id)
    fn drop(self)
        super.put_on()

fn main()
    let d = Derived(1)
    println(f"{d.id}")
"#;
    assert_escape_refused_in(source, "        super.put_on()");
}

#[test]
fn a_hook_binding_self_to_a_local_it_hands_to_a_keeper_is_refused_at_the_binding() {
    assert_escape_refused(&["let t = self", "keep(t)"], "        let t = self");
}

/// A hook declares no return type, so returning `self` is already a type
/// mismatch.
#[test]
fn a_hook_returning_self_is_refused() {
    assert_compiler_error(&hook_running(&["return self"]), "expected void, got Tag");
}

#[test]
fn a_generic_class_hook_letting_self_escape_is_refused() {
    let source = r#"
struct Shelf<T>
    slot Wrap<T>?

class Wrap<T>
    public var value T
    fn init(value T)
        self.value = value
    fn drop(self)
        let _s = Shelf<T>(slot: Some(self))

fn main()
    let w = Wrap<int>(1)
    println(f"{w.value}")
"#;
    assert_escape_refused_in(source, "let _s = Shelf<T>(slot: Some(self))");
}

#[test]
fn an_enum_hook_letting_self_escape_is_refused() {
    let source = r#"
struct Shelf
    slot Res?

enum Res
    A(int)
    B

    fn drop(self)
        let _s = Shelf(slot: Some(self))
        println("closed res")

fn main()
    let r = Res.A(1)
    println("made")
"#;
    assert_escape_refused_in(source, "let _s = Shelf(slot: Some(self))");
}

/// An enum's hook that only reads its receiver still runs, once.
#[test]
fn an_enum_hook_reading_self_runs() {
    assert_heap_guard_output(
        r#"
enum Res
    A(int)
    B

    fn drop(self)
        match self
            Res.A(n): println(f"closed {n}")
            Res.B: println("closed b")

fn main()
    let r = Res.A(1)
    println("made")
"#,
        "made\nclosed 1",
    );
}

#[test]
fn a_hook_handing_self_to_an_imported_keeper_is_refused() {
    let library = r#"
public class Shelf
    public var slot Tag?
    fn init()
        self.slot = None

public class Tag
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        keep(self)

public fn keep(t Tag) int
    var s = Shelf()
    s.slot = Some(t)
    return 0
"#;
    let main = "use system.io\nuse local.tags.{Tag}\n\nfn main()\n    let a = Tag(1)\n    println(f\"{a.id}\")\n";
    let files = [("main.mi", main), ("tags.mi", library)];
    assert_project_compiler_error(&files, CODE);
    assert_project_compiler_error(&files, "tags.mi:");
}

/// A hook that reads `self`'s fields, lends `self` to a function and a
/// method that only read it, and lends a resource field still runs, and
/// releases each instance once.
#[test]
fn a_hook_reading_and_lending_self_runs() {
    assert_heap_guard_output(
        &hook_running(&[
            "println(f\"dropped {self.id}\")",
            "println(f\"look {look(self)}\")",
            "println(f\"describe {self.describe()}\")",
            "println(f\"looker {Looker().take(self)}\")",
            "println(f\"peek {peek(self.inner)}\")",
        ]),
        "1\ndropped 1\nlook 1\ndescribe 1\nlooker 1\npeek 1\ninner 1",
    );
}
