// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the copy a body binds on entry of a parameter it writes.
//!
//! Each program is lowered through the whole pipeline, reference counting and
//! the choice of which parameters a function takes over included, so the
//! assertions read the MIR code generation is handed.

use miri::mir::{Body, Local, Operand, Place, Rvalue, StatementKind};
use miri::pipeline::Pipeline;

/// The body `source` lowers the function `name` to.
fn lowered(source: &str, name: &str) -> Body {
    Pipeline::new()
        .get_mir_bodies_with_rc(source)
        .expect("pipeline should lower the program")
        .into_iter()
        .find(|(symbol, _)| symbol == &format!("miri.{name}"))
        .map(|(_, body)| body)
        .unwrap_or_else(|| panic!("no body for `{name}`"))
}

/// The locals of `body` named `name`.
fn locals_named(body: &Body, name: &str) -> Vec<Local> {
    (0..body.local_decls.len())
        .map(Local)
        .filter(|local| body.local_decls[local.0].name.as_deref() == Some(name))
        .collect()
}

/// Whether `body` retains or releases the whole of `local` anywhere.
fn counts(body: &Body, local: Local) -> bool {
    body.basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .any(|statement| {
            matches!(&statement.kind,
                StatementKind::IncRef(place) | StatementKind::DecRef(place)
                    if *place == Place::new(local))
        })
}

const WRITING: &str = r#"
use system.io
use system.collections.list

fn f(xs [int]) int
    xs.push(7)
    xs.length()

fn main()
    var a = List([1, 2, 3])
    println(f"{f(a)}")
"#;

#[test]
fn test_a_written_parameter_is_moved_into_a_copy_on_entry() {
    let body = lowered(WRITING, "f");
    let copy = locals_named(&body, "xs")
        .into_iter()
        .find(|local| local.0 > body.arg_count)
        .expect("the body binds a copy of `xs`");
    let entry = &body.basic_blocks[0].statements;
    assert!(
        matches!(&entry[0].kind, StatementKind::StorageLive(place) if *place == Place::new(copy)),
        "{entry:?}"
    );
    assert!(
        matches!(&entry[1].kind,
            StatementKind::Assign(dest, Rvalue::Use(Operand::Move(source)))
                if *dest == Place::new(copy) && *source == Place::new(Local(1))),
        "{entry:?}"
    );
    assert_eq!(body.owned_params.first(), Some(&true));
}

#[test]
fn test_a_parameter_only_read_is_not_copied() {
    let source = r#"
use system.io
use system.collections.list

fn sum(xs [int]) int
    var total = 0
    for x in xs
        total += x
    total

fn main()
    let a = List([1, 2, 3])
    println(f"{sum(a)}")
"#;
    let body = lowered(source, "sum");
    assert_eq!(locals_named(&body, "xs"), vec![Local(1)]);
    assert!(!counts(&body, Local(1)));
}

#[test]
fn test_a_parameter_passed_on_to_a_writing_callee_is_not_copied() {
    let source = r#"
use system.io
use system.collections.list

fn inner(xs [int]) int
    xs.push(1)
    xs.length()

fn outer(xs [int]) int
    inner(xs)

fn main()
    let a = List([1, 2, 3])
    println(f"{outer(a)}")
"#;
    let body = lowered(source, "outer");
    assert_eq!(locals_named(&body, "xs"), vec![Local(1)]);
}

#[test]
fn test_an_out_parameter_written_through_is_not_copied() {
    let source = r#"
use system.io
use system.collections.list

struct S
    items [int]

fn add(s out S)
    s.items.push(99)

fn main()
    var s = S(items: List([1, 2]))
    add(s)
    println(f"{s.items.length()}")
"#;
    let body = lowered(source, "add");
    assert_eq!(locals_named(&body, "s"), vec![Local(1)]);
}
