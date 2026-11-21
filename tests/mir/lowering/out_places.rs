// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How a call hands a place to an `out` parameter, as lowering spells it.
//!
//! Device code hands every `out` argument over copied in: the place's value
//! is copied into a temp of its own before the call, the temp is handed
//! over, and the temp is copied back into the place as the first statement
//! of the block the call continues in. Device code holds no counts and no
//! copy-on-write block, so nothing on the path is made the caller's own.

use crate::mir::utils::{lower_named, lower_named_amended, mir_lower_code, out_handovers};
use miri::ast::expression::ExpressionKind;
use miri::ast::statement::{Statement, StatementKind};
use miri::mir::TerminatorKind;

const BUMPS: &str = "
use system.collections.array

struct Point
    x int
    y int

fn bump(n out int)
    n = n + 1

fn main()
    var p = Point(x: 1, y: 2)
    var a = [1, 2, 3]
    var k = 4
    var i = 1
    bump(p.x)
    bump(a[i])
    bump(k)
";

#[test]
fn device_code_copies_every_out_argument_in_and_stores_it_back() {
    let body = lower_named(BUMPS, "main", true);
    let found = out_handovers(&body);
    assert_eq!(found.len(), 3, "three out arguments:\n{body}");
    for handover in &found {
        assert!(
            handover.operand.projection.is_empty(),
            "an out operand in device code is a whole temp, not `{}`:\n{body}",
            handover.operand
        );
        let from = handover.copied_from.as_ref();
        assert!(
            from.is_some(),
            "no copy-in for `{}`:\n{body}",
            handover.operand
        );
        assert_eq!(
            from,
            handover.stored_back_into.as_ref(),
            "the value is stored back into the place it was copied from:\n{body}"
        );
    }
    let projected = found
        .iter()
        .filter_map(|h| h.copied_from.as_ref())
        .filter(|place| !place.projection.is_empty())
        .count();
    assert_eq!(
        projected, 2,
        "`p.x` and `a[i]` are projected places:\n{body}"
    );
}

#[test]
fn device_code_makes_nothing_on_an_out_path_its_own() {
    let body = lower_named(BUMPS, "main", true);
    let branches = (body.basic_blocks.iter())
        .filter(|b| {
            matches!(
                b.terminator.as_ref().map(|t| &t.kind),
                Some(TerminatorKind::SwitchInt { .. })
            )
        })
        .count();
    assert_eq!(
        branches, 0,
        "no copy-on-write check in device code:\n{body}"
    );
}

#[test]
fn a_host_body_hands_a_counted_slot_by_its_address() {
    let source = "
use system.collections.list

fn grow(xs out List<int>)
    xs.push(1)

fn main()
    var all = List([List([1]), List([2])])
    var i = 1
    grow(all[i])
";
    let body = mir_lower_code(source);
    let found = out_handovers(&body);
    assert_eq!(found.len(), 1, "{body}");
    assert!(
        !found[0].operand.projection.is_empty(),
        "the host hands a counted element by its slot:\n{body}"
    );
}

/// The id of the call to `callee` written as a statement of the function
/// `function` declares, and the id of the call's first argument.
fn call_and_first_argument(function: &Statement, callee: &str) -> (usize, usize) {
    let StatementKind::FunctionDeclaration(decl) = &function.node else {
        panic!("not a function declaration");
    };
    let Some(StatementKind::Block(statements)) = decl.body.as_ref().map(|b| &b.node) else {
        panic!("`{}` has no block body", decl.name);
    };
    statements
        .iter()
        .find_map(|statement| match &statement.node {
            StatementKind::Expression(expr) => match &expr.node {
                ExpressionKind::Call(func, args) => match &func.node {
                    ExpressionKind::Identifier(name, _) if name.as_str() == callee => {
                        Some((expr.id, args[0].id))
                    }
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .unwrap_or_else(|| panic!("no call to `{callee}`"))
}

const LENT: &str = "
fn note(v int)
    let _ = v + 1

fn helper(k int)
    var n = k
    note(n)

fn main()
    helper(1)
";

#[test]
fn device_code_lowers_a_call_the_record_agrees_with() {
    lower_named_amended(LENT, ("helper", true), |_, _| {}).expect("lowers");
}

/// Lowering reads which parameters are `out` off the callee's declaration
/// and the checker records which arguments bind one: in device code, where
/// the record decides a buffer is written, the two must agree.
#[test]
fn device_code_refuses_a_call_the_record_says_binds_out_but_the_declaration_lends() {
    let lowered = lower_named_amended(LENT, ("helper", true), |checker, function| {
        let (call, argument) = call_and_first_argument(function, "note");
        checker.out_arguments.record(call, argument);
    });
    let error = lowered.err().expect("a disagreement is an internal error");
    let text = format!("{error:?}");
    assert!(text.contains("internal compiler error"), "{text}");
    assert!(text.contains("`out`"), "{text}");
}

/// The host keeps its own answer: the record is read only in device code.
#[test]
fn a_host_body_lowers_whatever_the_record_says() {
    let lowered = lower_named_amended(LENT, ("helper", false), |checker, function| {
        let (call, argument) = call_and_first_argument(function, "note");
        checker.out_arguments.record(call, argument);
    });
    lowered.expect("the host reads the declaration alone");
}
