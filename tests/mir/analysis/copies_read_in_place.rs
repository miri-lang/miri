// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::mir::analysis::copies_read_in_place::copies_read_in_place;
use miri::mir::{Body, Local, Operand, PlaceElem, Rvalue, StatementKind};

use crate::mir::utils::mir_lower_code;

const OPS: &str = "
use system.io
use system.collections.list

enum Op
    Add(int, int)
    Halt
";

fn lower(main: &str) -> Body {
    mir_lower_code(&format!("{OPS}{main}"))
}

/// Every local a statement assigns a whole element of a collection: `_t = _c[_i]`.
fn element_copies(body: &Body) -> Vec<Local> {
    body.basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            StatementKind::Assign(dest, Rvalue::Use(Operand::Copy(source)))
                if dest.projection.is_empty()
                    && matches!(source.projection.last(), Some(PlaceElem::Index(_))) =>
            {
                Some(dest.local)
            }
            _ => None,
        })
        .collect()
}

/// The one element copy of `body`, and whether it is read in place.
fn element_copy_is_read_in_place(body: &Body) -> bool {
    let copies = element_copies(body);
    let [copy] = copies.as_slice() else {
        panic!("expected one element copy, found {copies:?}:\n{body}");
    };
    copies_read_in_place(body).contains(copy)
}

#[test]
fn test_a_match_on_a_held_element_reads_it_in_place() {
    let body = lower(
        "
fn main()
    var code = List([Op.Add(1, 2), Op.Halt])
    var i = 0
    match code[i]
        Op.Add(a, b)
            code[i] = Op.Halt
            println(f\"{a + b}\")
        Op.Halt: i = 0
",
    );
    assert!(
        element_copy_is_read_in_place(&body),
        "every read of the copy precedes the write:\n{body}"
    );
}

/// A binding's storage starting before its copy reads nothing, so a bound
/// copy read before anything writes is read in place as a temp is.
#[test]
fn test_a_bound_copy_read_before_any_write_is_read_in_place() {
    let body = lower(
        "
fn main()
    let code = List([Op.Add(1, 2), Op.Halt])
    var i = 0
    let op = code[i]
    match op
        Op.Add(a, b): i = a + b
        Op.Halt: i = 0
",
    );
    assert!(element_copy_is_read_in_place(&body), "{body}");
}

#[test]
fn test_a_guard_that_writes_nothing_keeps_the_copy_in_place() {
    let body = lower(
        "
fn main()
    let code = List([Op.Add(1, 2), Op.Halt])
    let limits = List([5, 1])
    var i = 0
    match code[i]
        Op.Add(a, b) if a < limits[i]: i = a
        Op.Add(a, b): i = b
        Op.Halt: i = 0
",
    );
    assert!(element_copy_is_read_in_place(&body), "{body}");
}

#[test]
fn test_a_copy_read_after_its_collection_is_written_is_a_real_copy() {
    let body = lower(
        "
fn main()
    var code = List([Op.Add(1, 2), Op.Halt])
    let op = code[0]
    code[0] = Op.Halt
    match op
        Op.Add(a, b): println(f\"{a + b}\")
        Op.Halt: println(\"halt\")
",
    );
    assert!(
        !element_copy_is_read_in_place(&body),
        "the write lands between the copy and its reads:\n{body}"
    );
}

#[test]
fn test_a_copy_read_after_its_index_changes_is_a_real_copy() {
    let body = lower(
        "
fn main()
    let code = List([Op.Add(1, 2), Op.Halt])
    var i = 0
    let op = code[i]
    i = 1
    match op
        Op.Add(a, b): i = a + b
        Op.Halt: i = 0
",
    );
    assert!(
        !element_copy_is_read_in_place(&body),
        "the index names another element by the time the copy is read:\n{body}"
    );
}

#[test]
fn test_a_copy_read_across_a_call_is_a_real_copy() {
    let body = lower(
        "
fn main()
    let code = List([Op.Add(1, 2), Op.Halt])
    let op = code[0]
    println(\"between\")
    match op
        Op.Add(a, b): println(f\"{a + b}\")
        Op.Halt: println(\"halt\")
",
    );
    assert!(
        !element_copy_is_read_in_place(&body),
        "a call may change any memory:\n{body}"
    );
}
