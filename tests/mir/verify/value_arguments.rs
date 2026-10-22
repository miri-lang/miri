// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rule that a value held as its bytes is handed to a call as a
//! whole local the call does not also write, and is never handed over owned.
//!
//! Each broken fixture is checked twice: as built, and as the copy pass
//! leaves it.

use crate::mir::value_bodies::*;
use miri::mir::value_arguments::copy_value_arguments;
use miri::mir::verify::{verify_value_arguments, VerificationViolation};
use miri::mir::{ArgMode, Body, Operand};

fn messages(violations: &[VerificationViolation]) -> String {
    violations
        .iter()
        .map(VerificationViolation::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// `body` raises exactly one finding carrying `fragment`.
fn assert_one_finding(body: &Body, fragment: &str) {
    let violations = verify_value_arguments(body);
    let text = messages(&violations);
    assert_eq!(violations.len(), 1, "expected one finding, got: {text}");
    assert!(text.contains(fragment), "{text}");
}

/// `body` raises exactly one finding carrying `fragment`, and none once the
/// copy pass has run over it.
fn assert_flagged_until_copied(mut body: Body, fragment: &str) {
    assert_one_finding(&body, fragment);
    copy_value_arguments(&mut body);
    let after = verify_value_arguments(&body);
    assert!(after.is_empty(), "{}", messages(&after));
}

#[test]
fn a_value_argument_read_from_inside_its_holder_is_flagged() {
    let args = vec![Operand::Copy(field(3, 0))];
    assert_flagged_until_copied(
        caller(call(args, Vec::new(), place(0))),
        "from inside its holder",
    );
}

#[test]
fn a_value_argument_the_call_writes_as_its_destination_is_flagged() {
    assert_flagged_until_copied(
        caller(call(vec![copy(1)], Vec::new(), place(1))),
        "as its destination",
    );
}

#[test]
fn a_value_argument_the_call_writes_through_out_is_flagged() {
    let args = vec![Operand::Copy(field(1, 0)), copy(1)];
    let modes = vec![ArgMode::Out, ArgMode::Borrowed];
    assert_flagged_until_copied(
        caller(call(args, modes, place(0))),
        "through an `out` argument",
    );
}

#[test]
fn a_value_argument_passed_owned_is_flagged() {
    let body = caller(call(vec![copy(1)], vec![ArgMode::Owned], place(0)));
    assert_one_finding(&body, "passed owned");
}

#[test]
fn distinct_whole_value_locals_raise_nothing() {
    let modes = vec![ArgMode::Out, ArgMode::Borrowed, ArgMode::Borrowed];
    let body = caller(call(vec![copy(2), copy(1), copy(1)], modes, place(0)));
    let violations = verify_value_arguments(&body);
    assert!(violations.is_empty(), "{}", messages(&violations));
}
