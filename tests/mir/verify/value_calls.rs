// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rule that a call through a function value owns exactly the
//! positions its value's type hands a reference on at, that every closure
//! body owns exactly its convention's parameters, and that a closure handed
//! to a runtime closure slot either owns nothing or is an adapter lending
//! every argument.

use miri::mir::verify::{verify_value_calls, VerificationViolation};
use miri::mir::{
    AggregateKind, ArgMode, Body, Local, Operand, Place, Rvalue, StatementKind, TerminatorKind,
};
use miri::pipeline::Pipeline;
use miri::type_checker::context::TypeDefinition;
use std::collections::HashMap;

const VALUES: &str = r#"
use system.collections.list

struct Shelf
    slot Handle?

class Handle
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

fn keep(h Handle, n int) int
    if true
        let _shelf = Shelf(slot: Some(h))
    return n

@writes("l")
@closure("f, 2, l")
runtime "core" fn miri_rt_list_sort_by(l [Handle], f fn(Handle, Handle) int)

fn main()
    let f = fn(h Handle, n int) int
        println(f"read {h.id}")
        return n
    let a = f(Handle(1), 1)
    let k = keep
    let b = k(Handle(2), 2)
    fn countdown(h Handle, n int) int
        if n > 0
            countdown(Handle(h.id + 1), n - 1)
        return 0
    let c = countdown(Handle(3), 1)
    var l = List<Handle>()
    l.push(Handle(5))
    let cmp = fn(x Handle, y Handle) int: x.id - y.id
    miri_rt_list_sort_by(l, cmp)
    println(f"{a} {b} {c}")
"#;

/// The bodies the MIR passes leave for [`VALUES`], with the type table they
/// were checked against.
fn compiled() -> (Vec<(String, Body)>, HashMap<String, TypeDefinition>) {
    let pipeline = Pipeline::new();
    let (bodies, _) = pipeline
        .get_mir_program_with_rc(VALUES)
        .expect("the fixture compiles");
    let checked = pipeline
        .frontend_script(VALUES)
        .expect("the fixture type checks");
    (bodies, checked.type_checker.type_definitions().clone())
}

fn violations_in(
    bodies: &[(String, Body)],
    definitions: &HashMap<String, TypeDefinition>,
) -> Vec<(String, VerificationViolation)> {
    verify_value_calls(bodies, definitions)
}

/// Asserts `found` is exactly `count` violations, each in a body whose name
/// contains `body` and whose message contains `message`.
fn assert_found(
    found: &[(String, VerificationViolation)],
    count: usize,
    body: &str,
    message: &str,
) {
    assert_eq!(found.len(), count, "{found:#?}");
    for (name, violation) in found {
        assert!(
            name.contains(body),
            "found in `{name}`, not `{body}`: {found:#?}"
        );
        let text = violation.to_string();
        assert!(text.contains(message), "`{text}` does not say `{message}`");
    }
}

fn is_main(name: &str) -> bool {
    name.ends_with(".main") || name == "main"
}

fn main_body(bodies: &mut [(String, Body)]) -> &mut Body {
    (bodies.iter_mut())
        .find(|(name, _)| is_main(name))
        .map(|(_, body)| body)
        .expect("a `main` body")
}

/// The modes of every call in `body` through a function value.
fn value_call_modes(body: &mut Body) -> Vec<&mut Vec<ArgMode>> {
    (body.basic_blocks.iter_mut())
        .filter_map(|block| block.terminator.as_mut())
        .filter_map(|terminator| match &mut terminator.kind {
            TerminatorKind::Call {
                func: Operand::Copy(_) | Operand::Move(_),
                arg_modes,
                ..
            } => Some(arg_modes),
            _ => None,
        })
        .collect()
}

/// The closure argument of the runtime sort call in `body`.
fn sort_closure_argument(body: &mut Body) -> &mut Operand {
    (body.basic_blocks.iter_mut())
        .filter_map(|block| block.terminator.as_mut())
        .find_map(|terminator| match &mut terminator.kind {
            TerminatorKind::Call { func, args, .. }
                if func.called_runtime_name() == Some("miri_rt_list_sort_by") =>
            {
                args.get_mut(1)
            }
            _ => None,
        })
        .expect("the runtime sort call")
}

const CONVENTION: &str = "against the value's convention";
const SIGNATURE: &str = "is a closure body whose convention owns the parameters";
const UNADAPTED: &str = "unadapted; the runtime lends every argument";
const ADAPTER: &str = "is handed to a runtime closure slot, which lends every argument";

#[test]
fn control_the_lowered_fixture_follows_every_convention() {
    let (bodies, definitions) = compiled();
    let closures = (bodies.iter())
        .filter(|(_, body)| body.captures.closure.is_some())
        .count();
    assert_eq!(
        closures, 5,
        "two lambdas, a thunk, a nested function and the sort's adapter"
    );
    assert_found(&violations_in(&bodies, &definitions), 0, "", "");
}

#[test]
fn a_call_through_a_value_lending_a_resource_is_refused() {
    let (mut bodies, definitions) = compiled();
    let main = main_body(&mut bodies);
    let modes = value_call_modes(main);
    assert_eq!(modes.len(), 3, "three calls through a value in\n{main}");
    for modes in modes {
        assert_eq!(modes[0], ArgMode::Owned);
        modes[0] = ArgMode::Borrowed;
    }
    assert_found(&violations_in(&bodies, &definitions), 3, "main", CONVENTION);
}

#[test]
fn a_call_through_a_value_handing_over_a_plain_argument_is_refused() {
    let (mut bodies, definitions) = compiled();
    let main = main_body(&mut bodies);
    let mut modes = value_call_modes(main);
    modes[0][1] = ArgMode::Owned;
    assert_found(&violations_in(&bodies, &definitions), 1, "main", CONVENTION);
}

#[test]
fn a_closure_body_lending_a_resource_parameter_is_refused() {
    let (mut bodies, definitions) = compiled();
    let (name, lambda) = (bodies.iter_mut())
        .find(|(_, body)| body.signature.kept_params.get(1) == Some(&true))
        .expect("a closure body owning its first parameter");
    let name = name.clone();
    lambda.signature.kept_params.clear();
    assert_found(&violations_in(&bodies, &definitions), 1, &name, SIGNATURE);
}

#[test]
fn a_nested_function_calling_itself_lending_a_resource_is_refused() {
    let (mut bodies, definitions) = compiled();
    let (_, nested) = (bodies.iter_mut())
        .find(|(name, body)| body.captures.closure.is_some() && name.contains("countdown"))
        .expect("the nested function's body");
    let mut modes = value_call_modes(nested);
    assert_eq!(modes.len(), 1, "one call through its own value");
    modes[0][0] = ArgMode::Borrowed;
    assert_found(
        &violations_in(&bodies, &definitions),
        1,
        "countdown",
        CONVENTION,
    );
}

/// The closure `main` creates for the runtime sort: the place it is
/// created in and the statement creating it, by block and index.
fn sort_adapter(main: &mut Body) -> (Place, usize, usize) {
    let Some(adapter) = sort_closure_argument(main).place().cloned() else {
        panic!("the sort is handed a place");
    };
    let found = (main.basic_blocks.iter().enumerate()).find_map(|(block, data)| {
        (data.statements.iter()).position(|statement| {
            matches!(&statement.kind, StatementKind::Assign(dest, Rvalue::Aggregate(AggregateKind::Closure(..), _))
                if *dest == adapter)
        })
        .map(|index| (block, index))
    });
    let (block, index) = found.expect("the adapter is created in `main`");
    (adapter, block, index)
}

#[test]
fn a_closure_owning_a_parameter_handed_to_a_runtime_slot_unadapted_is_refused() {
    let (mut bodies, definitions) = compiled();
    let main = main_body(&mut bodies);
    let cmp = (main.local_decls.iter())
        .position(|decl| decl.name.as_deref() == Some("cmp"))
        .expect("the comparator's binding");
    let (adapter, block, index) = sort_adapter(main);
    main.basic_blocks[block].statements[index].kind =
        StatementKind::Assign(adapter, Rvalue::Use(Operand::Copy(Place::new(Local(cmp)))));
    let found: Vec<_> = (violations_in(&bodies, &definitions).into_iter())
        .filter(|(name, _)| is_main(name))
        .collect();
    assert_found(&found, 1, "main", UNADAPTED);
}

#[test]
fn an_adapter_owning_a_parameter_is_refused() {
    let (mut bodies, definitions) = compiled();
    let main = main_body(&mut bodies);
    let (_, block, index) = sort_adapter(main);
    let StatementKind::Assign(_, Rvalue::Aggregate(AggregateKind::Closure(symbol, _), _)) =
        main.basic_blocks[block].statements[index].kind.clone()
    else {
        panic!("the adapter's creation");
    };
    let (name, body) = (bodies.iter_mut())
        .find(|(_, body)| body.captures.closure.as_ref() == Some(&symbol))
        .expect("the adapter's body");
    let name = name.clone();
    body.signature.kept_params = vec![false, true, true];
    assert_found(&violations_in(&bodies, &definitions), 1, &name, ADAPTER);
}

#[test]
fn an_adapter_created_after_the_runtime_call_is_refused() {
    let (mut bodies, definitions) = compiled();
    let main = main_body(&mut bodies);
    let (_, block, index) = sort_adapter(main);
    let Some(TerminatorKind::Call {
        target: Some(after),
        ..
    }) = main.basic_blocks[block]
        .terminator
        .as_ref()
        .map(|t| t.kind.clone())
    else {
        panic!("the runtime sort continues");
    };
    let creation = main.basic_blocks[block].statements.remove(index);
    main.basic_blocks[after.0].statements.insert(0, creation);
    let found: Vec<_> = (violations_in(&bodies, &definitions).into_iter())
        .filter(|(name, _)| is_main(name))
        .collect();
    assert_found(&found, 1, "main", UNADAPTED);
}
