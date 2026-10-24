// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for how a write reaches the receiver of the method making it.
//!
//! A method that writes a collection receiver is called on a collection its
//! caller made the method's own first, so the method checks that the
//! receiver arrived unique instead of copying it on write: a copy-on-write
//! call would release a reference the method never held. A receiver reached
//! through a field, a capture and an `out` parameter are not the method's own
//! receiver, and keep their copy-on-write. A method handing its receiver
//! straight to a runtime entry point that writes it in place checks it the
//! same way, and one handing it only to an entry point that reads it does
//! not.
//!
//! Each program is lowered through the whole pipeline, reference counting
//! included, so the assertions read the MIR code generation is handed.

use miri::mir::{Body, Local, Operand, Place, PlaceElem, TerminatorKind};
use miri::pipeline::Pipeline;
use miri::runtime_fns::{is_copy_on_write, rt};

/// Every body `source` lowers to, by its symbol.
fn lowered(source: &str) -> Vec<(String, Body)> {
    Pipeline::new()
        .get_mir_bodies_with_rc(source)
        .expect("pipeline should lower the program")
}

/// The first body whose symbol satisfies `is_wanted`.
fn body_where(bodies: &[(String, Body)], is_wanted: impl Fn(&str) -> bool) -> &Body {
    bodies
        .iter()
        .find(|(symbol, _)| is_wanted(symbol))
        .map(|(_, body)| body)
        .unwrap_or_else(|| {
            let names: Vec<&str> = bodies.iter().map(|(symbol, _)| symbol.as_str()).collect();
            panic!("no body found among {names:?}")
        })
}

/// The body of method `method` of the collection class `class`, at any
/// instantiation.
fn method_body<'b>(bodies: &'b [(String, Body)], class: &str, method: &str) -> &'b Body {
    body_where(bodies, |symbol| {
        let owner = symbol
            .strip_prefix("miri$system$collections$")
            .unwrap_or(symbol);
        (owner.starts_with(&format!("miri.{class}."))
            || owner.starts_with(&format!("miri.{class}$")))
            && symbol.ends_with(&format!(".{method}"))
    })
}

/// The argument lists of every runtime call `name` in `body`.
fn runtime_calls<'b>(body: &'b Body, is_named: impl Fn(&str) -> bool) -> Vec<&'b [Operand]> {
    body.basic_blocks
        .iter()
        .filter_map(|block| block.terminator.as_ref())
        .filter_map(|terminator| match &terminator.kind {
            TerminatorKind::Call { func, args, .. }
                if func.called_runtime_name().is_some_and(&is_named) =>
            {
                Some(args.as_slice())
            }
            _ => None,
        })
        .collect()
}

/// Whether some runtime call `name` in `body` is handed exactly `place`.
fn hands(body: &Body, is_named: impl Fn(&str) -> bool, place: &Place) -> bool {
    runtime_calls(body, is_named)
        .iter()
        .any(|args| matches!(args, [argument] if argument.place() == Some(place)))
}

fn receiver(body: &Body) -> Place {
    Place::new(body.receiver.expect("a method body names its receiver"))
}

fn is_receiver_check(name: &str) -> bool {
    name == rt::RECEIVER_UNIQUE
}

const COLLECTION_WRITERS: &str = r#"
use system.io
use system.collections.list

fn main()
    var l = List([3, 1, 2])
    l.resize(5, 0)
    var a = [1, 2, 3]
    a.reverse()
    println(f"{l.length()} {a[0]}")
"#;

#[test]
fn test_a_writer_calling_a_writer_on_its_list_receiver_checks_it_arrived_unique() {
    let bodies = lowered(COLLECTION_WRITERS);
    let body = method_body(&bodies, "List", "resize");
    let receiver = receiver(body);
    assert!(hands(body, is_receiver_check, &receiver));
    assert!(!hands(body, is_copy_on_write, &receiver));
}

#[test]
fn test_a_writer_calling_a_writer_on_its_array_receiver_checks_it_arrived_unique() {
    let bodies = lowered(COLLECTION_WRITERS);
    let body = method_body(&bodies, "Array", "reverse");
    let receiver = receiver(body);
    assert!(hands(body, is_receiver_check, &receiver));
    assert!(!hands(body, is_copy_on_write, &receiver));
}

#[test]
fn test_a_write_through_a_field_of_the_receiver_still_copies_a_shared_collection() {
    let source = r#"
use system.io
use system.collections.list

class Holder
    var items List<int>

    fn add(x int)
        self.items.push(x)

fn main()
    let h = Holder(items: List([1]))
    h.add(2)
    println(f"{h.items.length()}")
"#;
    let bodies = lowered(source);
    let body = body_where(&bodies, |symbol| symbol == "miri.Holder.add");
    let mut items = receiver(body);
    items.projection.push(PlaceElem::Field(0));
    assert!(hands(body, |name| name == rt::RC_IS_SHARED, &items));
    assert!(hands(body, |name| name == rt::LIST_CLONE, &items));
    assert!(runtime_calls(body, is_receiver_check).is_empty());
}

#[test]
fn test_an_out_parameter_written_still_copies_a_shared_collection() {
    let source = r#"
use system.io
use system.collections.list

fn add(xs out [int])
    xs.push(1)

fn main()
    var l = List([1, 2])
    add(l)
    println(f"{l.length()}")
"#;
    let bodies = lowered(source);
    let body = body_where(&bodies, |symbol| symbol == "miri.add");
    assert!(hands(body, is_copy_on_write, &Place::new(Local(1))));
    assert!(runtime_calls(body, is_receiver_check).is_empty());
}

#[test]
fn test_a_captured_collection_written_still_copies_a_shared_collection() {
    let source = r#"
use system.io
use system.collections.list

fn main()
    var l = List([1, 2])
    let add = fn(x int): l.push(x)
    add(3)
    println(f"{l.length()}")
"#;
    let bodies = lowered(source);
    let body = body_where(&bodies, |symbol| symbol.contains("$lambda"));
    assert!(body.receiver.is_none());
    assert!(!runtime_calls(body, is_copy_on_write).is_empty());
    assert!(runtime_calls(body, is_receiver_check).is_empty());
}

const RUNTIME_WRITERS: &str = r#"
use system.io
use system.collections.list
use system.collections.map
use system.collections.set

fn main()
    var l = List([3, 1, 2])
    l.clear()
    var m = Map<int, int>()
    m.clear()
    var s = Set<int>()
    s.clear()
    println(f"{l.length()} {m.length()} {s.length()} {m.contains_key(1)}")
"#;

#[test]
fn test_a_method_handing_its_list_receiver_to_a_runtime_writer_checks_it_arrived_unique() {
    let bodies = lowered(RUNTIME_WRITERS);
    let body = method_body(&bodies, "List", "clear");
    let receiver = receiver(body);
    assert!(hands(body, is_receiver_check, &receiver));
    assert!(!hands(body, is_copy_on_write, &receiver));
}

#[test]
fn test_a_method_handing_its_map_and_set_receivers_to_a_runtime_writer_checks_them() {
    let bodies = lowered(RUNTIME_WRITERS);
    for class in ["Map", "Set"] {
        let body = method_body(&bodies, class, "clear");
        assert!(
            hands(body, is_receiver_check, &receiver(body)),
            "{class}.clear checks its receiver"
        );
    }
}

#[test]
fn test_a_method_handing_its_receiver_only_to_a_runtime_reader_checks_nothing() {
    let bodies = lowered(RUNTIME_WRITERS);
    let body = method_body(&bodies, "Map", "contains_key");
    assert!(runtime_calls(body, is_receiver_check).is_empty());
}

#[test]
fn test_a_runtime_writer_handed_a_local_copies_it_on_write_first() {
    let source = r#"
use system.io
use system.collections.list

runtime "core" fn miri_rt_list_clear(l [int])

fn main()
    var l = List([1, 2])
    let keep = l
    miri_rt_list_clear(l)
    println(f"{l.length()} {keep.length()}")
"#;
    let bodies = lowered(source);
    let body = body_where(&bodies, |symbol| symbol == "main");
    let cleared = runtime_calls(body, |name| name == rt::LIST_CLEAR);
    let [[argument]] = cleared.as_slice() else {
        panic!("one call clears the list: {cleared:?}");
    };
    let list = argument.place().expect("the list is a place").clone();
    assert!(hands(body, is_copy_on_write, &list));
}
