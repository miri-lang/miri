// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the check that the element and closure ABI passes changed a
//! body only by the expansions the runtime's parameter table spells: an
//! element as its address and byte count, an element result as caller
//! storage, a closure followed by its trampoline.
//!
//! Every body of a real program is translated and checked clean; each broken
//! translation is a clean one with one thing changed.

use miri::ast::literal::{IntegerLiteral, Literal};
use miri::error::syntax::Span;
use miri::mir::closure_abi::pass_closures_with_trampolines;
use miri::mir::element_abi::pass_elements_by_address;
use miri::mir::facts::ProgramFacts;
use miri::mir::passes::{
    run_body_pass_with_facts, PASS_CLOSURES_WITH_TRAMPOLINES, PASS_ELEMENTS_BY_ADDRESS,
};
use miri::mir::verify::{verify_abi_translation, verify_backend_bodies, VerificationViolation};
use miri::mir::{
    Body, Constant, Operand, Place, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};
use miri::pipeline::Pipeline;

/// Pushes elements of three widths, sorts through a closure, and reads map
/// keys and values back into a register, a value and a reference.
const PROGRAM: &str = r#"
use system.io
use system.collections.list
use system.collections.map

struct Point
    x int
    y int

fn main()
    var xs = List<int>()
    xs.push(3)
    xs.push(1)
    let sorted = xs.sorted_by(fn(a int, b int) int: a - b)
    let first = sorted[0]
    var points = List<Point>()
    points.push(Point(x: 1, y: 2))
    let p = points[0]
    var flags = List<bool>()
    flags.push(true)
    var ages = Map<String, int>()
    ages["ann"] = 4
    var places = Map<int, Point>()
    places[1] = Point(x: 3, y: 4)
    for name in ages
        println(name)
    let q = places.value_at(0)
    let n = ages.value_at(0)
    println(f"{first} {p.x} {flags[0]} {q.y} {n}")
    let has = ages.contains_key("ann")
    println(f"{has}")
"#;

/// Each body of `PROGRAM` as lowered, beside the same body once both ABI
/// passes have run over it.
fn translated() -> Vec<(String, Body, Body)> {
    let bodies = Pipeline::new()
        .get_mir_bodies_with_rc(PROGRAM)
        .expect("the program lowers");
    bodies
        .into_iter()
        .map(|(name, before)| {
            let mut after = before.clone();
            pass_elements_by_address(&mut after).expect("the element ABI applies");
            pass_closures_with_trampolines(&mut after).expect("the closure ABI applies");
            (name, before, after)
        })
        .collect()
}

fn messages(violations: &[VerificationViolation]) -> Vec<String> {
    violations.iter().map(ToString::to_string).collect()
}

/// The body of `main` before and after translation, and the block of its
/// first call the element ABI rewrote.
fn main_with_element_call() -> (Body, Body, usize) {
    let (_, before, after) = translated()
        .into_iter()
        .find(|(name, _, _)| name == "main")
        .expect("main is lowered");
    let block = (0..before.basic_blocks.len())
        .find(|&block| call_args(&before, block).len() != call_args(&after, block).len())
        .expect("main calls the runtime with an element");
    (before, after, block)
}

fn call_args(body: &Body, block: usize) -> Vec<Operand> {
    match body.basic_blocks[block]
        .terminator
        .as_ref()
        .map(|t| &t.kind)
    {
        Some(TerminatorKind::Call { args, .. }) => args.clone(),
        _ => Vec::new(),
    }
}

fn call_mut(body: &mut Body, block: usize) -> &mut TerminatorKind {
    let Some(Terminator { kind, .. }) = body.basic_blocks[block].terminator.as_mut() else {
        panic!("block {block} ends in no terminator");
    };
    kind
}

/// `after` raises a finding carrying `fragment`.
fn assert_flagged(before: &Body, after: &Body, fragment: &str) {
    let found = messages(&verify_abi_translation(before, after));
    assert!(
        found.iter().any(|m| m.contains(fragment)),
        "expected a finding carrying {fragment:?}, got {found:?}\n{after}"
    );
}

#[test]
fn test_every_translated_body_of_a_program_is_clean() {
    let bodies = translated();
    assert!(
        bodies.iter().any(|(_, before, after)| before != after),
        "the program exercises neither ABI pass"
    );
    for (name, before, after) in &bodies {
        let found = messages(&verify_abi_translation(before, after));
        assert!(found.is_empty(), "fn {name}: {found:?}\n{after}");
    }
}

#[test]
fn test_a_closure_translation_is_checked_clean() {
    let closures = translated()
        .into_iter()
        .filter(|(_, before, after)| {
            (0..before.basic_blocks.len()).any(|block| {
                call_args(after, block)
                    .iter()
                    .any(|arg| matches!(arg, Operand::Function(f) if f.symbol.trampoline_shape().is_some()))
            })
        })
        .count();
    assert!(closures > 0, "no body hands the runtime a closure");
}

/// A runtime call the passes skipped still hands its element over by value,
/// which is not what the runtime entry point takes.
#[test]
fn test_an_element_call_left_unspelled_is_flagged() {
    let (before, _, _) = main_with_element_call();
    assert_flagged(&before, &before.clone(), "missing from the spelled call");
}

#[test]
fn test_a_release_the_translation_added_is_flagged() {
    let (before, mut after, block) = main_with_element_call();
    let released = Place::new(miri::mir::Local(1));
    after.basic_blocks[block].statements.push(Statement {
        kind: StatementKind::DecRef(released),
        span: Span::default(),
    });
    assert_flagged(&before, &after, "statement the ABI does not add");
}

#[test]
fn test_a_statement_the_translation_dropped_is_flagged() {
    let (before, mut after, _) = main_with_element_call();
    let block = (0..before.basic_blocks.len())
        .find(|&block| !before.basic_blocks[block].statements.is_empty())
        .expect("main has a statement");
    after.basic_blocks[block].statements.remove(0);
    assert_flagged(&before, &after, "statement");
}

#[test]
fn test_a_wrong_byte_count_is_flagged() {
    let (before, mut after, block) = main_with_element_call();
    let TerminatorKind::Call { args, .. } = call_mut(&mut after, block) else {
        panic!("the element call is a call");
    };
    let count = (args.iter_mut())
        .find(|arg| matches!(arg, Operand::Constant(c) if matches!(c.literal, Literal::Integer(_))))
        .expect("the element is followed by its byte count");
    *count = Operand::Constant(Box::new(Constant {
        span: Span::default(),
        ty: miri::ast::types::Type::new(miri::ast::types::TypeKind::Int, Span::default()),
        literal: Literal::Integer(IntegerLiteral::I64(3)),
    }));
    assert_flagged(&before, &after, "byte count");
}

#[test]
fn test_an_element_handed_over_by_value_is_flagged() {
    let (before, mut after, block) = main_with_element_call();
    let original = call_args(&before, block);
    let TerminatorKind::Call { args, .. } = call_mut(&mut after, block) else {
        panic!("the element call is a call");
    };
    *args = original;
    assert_flagged(&before, &after, "argument");
}

#[test]
fn test_a_changed_callee_is_flagged() {
    let (before, mut after, block) = main_with_element_call();
    let other = (0..before.basic_blocks.len())
        .filter(|&b| b != block)
        .find_map(
            |b| match &before.basic_blocks[b].terminator.as_ref()?.kind {
                TerminatorKind::Call { func, .. } => Some(func.clone()),
                _ => None,
            },
        )
        .expect("main makes a second call");
    let TerminatorKind::Call { func, .. } = call_mut(&mut after, block) else {
        panic!("the element call is a call");
    };
    *func = other;
    assert_flagged(&before, &after, "call");
}

#[test]
fn test_an_address_of_another_place_is_flagged() {
    let (before, mut after, block) = main_with_element_call();
    let address = (after.basic_blocks[block].statements.iter_mut())
        .rev()
        .find_map(|statement| match &mut statement.kind {
            StatementKind::Assign(_, Rvalue::Ref(place)) => Some(place),
            _ => None,
        })
        .expect("the translation takes an address");
    *address = Place::new(miri::mir::Local(1));
    assert_flagged(&before, &after, "address");
}

#[test]
fn test_a_redirected_terminator_is_flagged() {
    let (before, mut after, _) = main_with_element_call();
    let block = (0..before.basic_blocks.len())
        .find(|&block| {
            matches!(
                before.basic_blocks[block]
                    .terminator
                    .as_ref()
                    .map(|t| &t.kind),
                Some(TerminatorKind::Goto { .. })
            )
        })
        .expect("main has a goto");
    let TerminatorKind::Goto { target } = call_mut(&mut after, block) else {
        panic!("the block ends in a goto");
    };
    target.0 = 0;
    assert_flagged(&before, &after, "terminator");
}

/// The program hands the check element results in each storage the pass
/// writes them to: the destination itself, the address of a value's bytes,
/// and a slot copied out in a block of its own.
#[test]
fn test_the_program_returns_elements_through_every_storage() {
    let bodies = translated();
    let returning = |body: &Body| {
        (body
            .basic_blocks
            .iter()
            .filter_map(|data| data.terminator.as_ref()))
        .filter(|t| {
            matches!(&t.kind, TerminatorKind::Call { func, .. }
                if func.called_runtime_effect().is_some_and(|e| e.returns_element_value()))
        })
        .count()
    };
    let calls: usize = bodies.iter().map(|(_, before, _)| returning(before)).sum();
    let copied = (bodies.iter())
        .filter(|(_, before, after)| after.basic_blocks.len() > before.basic_blocks.len())
        .count();
    assert!(
        calls > 0 && copied > 0,
        "calls {calls}, bodies with a copy-out block {copied}"
    );
}

/// A body the element ABI gave a block that copies a result slot out.
fn with_copy_out_block() -> (Body, Body) {
    let (_, before, after) = translated()
        .into_iter()
        .find(|(_, before, after)| after.basic_blocks.len() > before.basic_blocks.len())
        .expect("a result reaches its destination through a slot");
    (before, after)
}

#[test]
fn test_a_slot_never_copied_out_is_flagged() {
    let (before, mut after) = with_copy_out_block();
    let copy = before.basic_blocks.len();
    after.basic_blocks[copy].statements.clear();
    assert_flagged(&before, &after, "is not copied into");
}

#[test]
fn test_a_closure_without_its_trampoline_is_flagged() {
    let trampoline_in = |body: &Body| {
        (0..body.basic_blocks.len()).find_map(|block| {
            let position = call_args(body, block).iter().position(
                |arg| matches!(arg, Operand::Function(f) if f.symbol.trampoline_shape().is_some()),
            )?;
            Some((block, position))
        })
    };
    let (before, mut after, (block, position)) = translated()
        .into_iter()
        .find_map(|(_, before, after)| {
            let found = trampoline_in(&after)?;
            Some((before, after, found))
        })
        .expect("a body hands the runtime a closure");
    let TerminatorKind::Call { args, .. } = call_mut(&mut after, block) else {
        panic!("the closure call is a call");
    };
    args[position] = args[position - 1].clone();
    assert_flagged(&before, &after, "not followed by a trampoline");
}

/// The bodies of `PROGRAM` as the backend receives them, with their facts
/// carried through both ABI passes, beside the bodies as lowered.
fn backend_program() -> (Vec<(String, Body)>, Vec<(String, Body)>, ProgramFacts) {
    let (lowered, mut facts) = Pipeline::new()
        .get_mir_program_with_rc(PROGRAM)
        .expect("the program lowers");
    let mut bodies = lowered.clone();
    for ((_, body), facts) in bodies.iter_mut().zip(facts.iter_mut()) {
        let elements = |body: &mut Body, _: &mut _| pass_elements_by_address(body);
        run_body_pass_with_facts(&PASS_ELEMENTS_BY_ADDRESS, body, facts, elements)
            .expect("the element ABI keeps its declaration")
            .expect("the element ABI applies");
        let closures = |body: &mut Body, _: &mut _| pass_closures_with_trampolines(body);
        run_body_pass_with_facts(&PASS_CLOSURES_WITH_TRAMPOLINES, body, facts, closures)
            .expect("the closure ABI keeps its declaration")
            .expect("the closure ABI applies");
    }
    (lowered, bodies, facts)
}

fn backend_report(
    bodies: &[(String, Body)],
    facts: &ProgramFacts,
    lowered: &[(String, Body)],
) -> Vec<String> {
    let result = Pipeline::new()
        .frontend_script(PROGRAM)
        .expect("the program checks");
    verify_backend_bodies(
        result.type_checker.type_definitions(),
        bodies,
        facts,
        Some(lowered),
    )
    .expect("the ABI passes keep every body's ownership facts current")
    .lines
}

#[test]
fn test_the_backend_checks_pass_a_translated_program() {
    let (lowered, bodies, facts) = backend_program();
    assert_eq!(
        backend_report(&bodies, &facts, &lowered),
        Vec::<String>::new()
    );
}

#[test]
fn test_the_backend_checks_find_a_mistranslated_body() {
    let (lowered, mut bodies, facts) = backend_program();
    let (name, main) = (bodies.iter_mut())
        .find(|(name, _)| name == "main")
        .expect("main is lowered");
    main.basic_blocks[0].statements.push(Statement {
        kind: StatementKind::DecRef(Place::new(miri::mir::Local(1))),
        span: Span::default(),
    });
    let name = name.clone();
    let report = backend_report(&bodies, &facts, &lowered);
    assert!(
        report
            .iter()
            .any(|line| line.contains(&format!("fn {name}:"))
                && line.contains("statement the ABI does not add")),
        "{report:?}"
    );
}

/// A body of the program, translated, whose call ending `block` looks up the
/// whole local `key` at element position `position`.
fn lookup_by_a_local() -> (Body, Body, usize, Place) {
    for (_, before, after) in translated() {
        for block in 0..before.basic_blocks.len() {
            let Some(TerminatorKind::Call { func, args, .. }) = before.basic_blocks[block]
                .terminator
                .as_ref()
                .map(|t| &t.kind)
            else {
                continue;
            };
            let Some(effect) = func.called_runtime_effect() else {
                continue;
            };
            let looked_up = (effect.element_positions().into_iter())
                .filter(|&position| !effect.stores(position))
                .find_map(|position| match args.get(position) {
                    Some(Operand::Copy(place)) if place.projection.is_empty() => {
                        Some(place.clone())
                    }
                    _ => None,
                });
            if let Some(key) = looked_up {
                return (before, after, block, key);
            }
        }
    }
    panic!("no body looks an element up by a local");
}

/// A copy of a looked-up element into a temporary is no expansion the ABI
/// spells: only a stored element travels from a copy the pass makes.
#[test]
fn test_a_looked_up_element_copied_into_a_temporary_is_flagged() {
    let (before, mut after, block, key) = lookup_by_a_local();
    let decl = after.local_decls[key.local.0].clone();
    let temporary = after.new_local(decl);
    let statements = &mut after.basic_blocks[block].statements;
    let address = (statements.iter_mut())
        .find_map(|statement| match &mut statement.kind {
            StatementKind::Assign(_, Rvalue::Ref(place)) if *place == key => Some(place),
            _ => None,
        })
        .expect("the translation takes the key's address");
    *address = Place::new(temporary);
    let copy = Statement {
        kind: StatementKind::Assign(
            Place::new(temporary),
            Rvalue::Use(Operand::Copy(key.clone())),
        ),
        span: Span::default(),
    };
    let at = before.basic_blocks[block].statements.len();
    after.basic_blocks[block].statements.insert(at, copy);
    assert_flagged(&before, &after, "not of the element");
}
