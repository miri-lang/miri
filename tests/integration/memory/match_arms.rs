// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for memory correctness in match expressions.
// Each match arm may create or alias managed objects; only the taken arm
// executes, so managed locals from non-taken arms must never generate
// phantom DecRef operations, and the taken arm's temporaries must be freed
// promptly when the arm expression is consumed.

use super::super::utils::*;

/// Each arm creates a temporary List; only the taken arm runs.
/// The temporary must be freed immediately after the arm expression completes.
#[test]
fn test_match_temp_list_per_arm_no_leak() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn label(n int) int
    match n
        1: List([1, 2, 3]).length()
        2: List([10, 20]).length()
        _: List([]).length()

fn main()
    println(f"{label(1)}")
    println(f"{label(2)}")
    println(f"{label(5)}")
"#,
        "3\n2\n0",
    );
}

/// Only the taken arm allocates; non-taken arms must not generate phantom frees.
#[test]
fn test_match_only_taken_arm_allocates_no_leak() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn maybe_alloc(flag int) int
    match flag
        1: List([1, 2, 3, 4, 5]).length()
        _: 0

fn main()
    println(f"{maybe_alloc(1)}")
    println(f"{maybe_alloc(0)}")
"#,
        "5\n0",
    );
}

/// Outer managed variable referenced in the match result expression must survive.
#[test]
fn test_match_outer_list_used_in_arm_no_leak() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let items = List([10, 20, 30])
    let n = match items.length()
        3: items.length() + 1
        _: 0
    println(f"{n}")
    println(f"{items.length()}")
"#,
        "4\n3",
    );
}

/// Class instance created as a temporary inside a match arm; must be freed at arm end.
#[test]
fn test_match_class_temp_in_arm_no_leak() {
    assert_runs_with_output(
        r#"

class Tag
    var code int

fn classify(n int) int
    match n
        1: Tag(code: 100).code
        2: Tag(code: 200).code
        _: Tag(code: 0).code

fn main()
    println(f"{classify(1)}")
    println(f"{classify(2)}")
    println(f"{classify(5)}")
"#,
        "100\n200\n0",
    );
}

/// Class with a managed List field created inline in a match arm; both the class
/// and the List must be freed when the arm result is consumed.
#[test]
fn test_match_class_with_list_field_in_arm_no_leak() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Packet
    var data [int]

fn process(flag int) int
    match flag
        1: Packet(data: List([1, 2, 3])).data.length()
        _: Packet(data: List([])).data.length()

fn main()
    println(f"{process(1)}")
    println(f"{process(0)}")
"#,
        "3\n0",
    );
}

/// Match selects one of two outer managed variables; neither must be dropped early.
/// Both must survive until the enclosing scope exits.
#[test]
fn test_match_selects_between_two_lists_no_leak() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let a = List([1, 2])
    let b = List([3, 4, 5])
    let chosen = match 0
        1: a
        _: b
    println(f"{chosen.length()}")
    println(f"{a.length()}")
    println(f"{b.length()}")
"#,
        "3\n2\n3",
    );
}

/// Match inside a loop: per-iteration temporaries must not accumulate.
#[test]
fn test_match_in_loop_no_accumulation() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn score(n int) int
    match n
        0: List([0]).length()
        _: List([n, n * 2]).length()

fn main()
    var total = 0
    for i in 0..5
        total = total + score(i)
    println(f"{total}")
"#,
        "9",
    );
}

/// Match on a bool; the taken arm creates a temporary class, the other does not.
#[test]
fn test_match_on_bool_arm_allocates_no_leak() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn check(flag bool) int
    match flag
        true: List([1, 2, 3]).length()
        false: 0

fn main()
    println(f"{check(true)}")
    println(f"{check(false)}")
"#,
        "3\n0",
    );
}

/// Match on an int with multiple arms; each arm creates and immediately
/// drops a class instance. Called repeatedly to catch accumulation.
#[test]
fn test_match_multiple_class_arms_called_repeatedly_no_leak() {
    assert_runs_with_output(
        r#"

class Node
    var value int

fn pick(n int) int
    match n
        0: Node(value: 0).value
        1: Node(value: 10).value
        2: Node(value: 20).value
        _: Node(value: 99).value

fn main()
    var sum = 0
    for i in 0..4
        sum = sum + pick(i)
    println(f"{sum}")
"#,
        "129",
    );
}

/// Matching on a variable holding an enum must not release its payload twice.
///
/// The match reads the subject into a temp and releases both when it ends, so
/// reading it has to retain it. Without that, one allocation carries two
/// releases and the heap is corrupted after enough iterations — a single
/// iteration usually survives, which is why this loops.
#[test]
fn test_match_on_named_enum_local_releases_payload_once() {
    assert_runs_with_output(
        r#"
var index = 0
var total = 0
while index < 300
    let value = Some(f"{index}")
    match value
        Some(text): total = total + text.length()
        None: total = total + 0
    index = index + 1
println(f"{total}")
"#,
        "790",
    );
}

/// The same release path, for a user-declared enum carrying an allocated
/// string rather than an `Option`.
#[test]
fn test_match_on_named_user_enum_local_releases_payload_once() {
    assert_runs_with_output(
        r#"
enum Token
    Word(String)
    Empty

var index = 0
var total = 0
while index < 300
    let token = Token.Word(f"{index}")
    match token
        Token.Word(text): total = total + text.length()
        Token.Empty: total = total + 0
    index = index + 1
println(f"{total}")
"#,
        "790",
    );
}

/// An arm binding that is returned must outlive the arm that bound it.
///
/// The binding is released when the arm's scope ends, so handing it to the
/// result has to retain it. Moving it there instead frees the value the caller
/// receives.
#[test]
fn test_returned_arm_binding_outlives_the_match() {
    assert_runs_with_output(
        r#"
enum Token
    Word(String)
    Empty

fn text_of(token Token) String
    match token
        Token.Word(text): text
        Token.Empty: ""

var index = 0
var total = 0
while index < 300
    total = total + text_of(Token.Word(f"{index}")).length()
    index = index + 1
println(f"{total}")
"#,
        "790",
    );
}

/// A match-arm binding handed to a container that stores it stays alive.
///
/// The arm's scope releases the binding when the arm ends, so the container
/// needs a reference of its own. Lowering donates one at the call site; without
/// it the map is left holding a freed string that reads back empty rather than
/// crashing.
#[test]
fn test_arm_binding_stored_by_callee_survives_the_arm() {
    assert_runs_with_output(
        r#"
use system.collections.map

var entries = Map<String, int>()
match Some("alpha" + "!")
    Some(key)
        entries.set(key, 1)
    None: println("none")
for stored in entries
    println(stored)
"#,
        "alpha!",
    );
}

/// The same guarantee for `Set.add`, which donates its element the same way.
#[test]
fn test_arm_binding_stored_in_a_set_survives_the_arm() {
    assert_runs_with_output(
        r#"
use system.collections.set

var entries = Set<String>()
match Some("beta" + "!")
    Some(element)
        entries.add(element)
    None: println("none")
for stored in entries
    println(stored)
"#,
        "beta!",
    );
}

/// Storing an arm binding repeatedly must neither leak nor double-free.
#[test]
fn test_arm_binding_stored_in_a_loop_stays_balanced() {
    assert_runs_with_output(
        r#"
use system.collections.map

var entries = Map<String, int>()
var index = 0
while index < 300
    match Some(f"key_{index}")
        Some(key)
            entries.set(key, index)
        None: println("none")
    index = index + 1
println(f"{entries.length()}")
"#,
        "300",
    );
}

/// An arm that returns leaves the match through a different exit than the
/// arms that fall through to the join. Both exits have to unwind the same
/// number of scopes, or every binding the enclosing scopes still hold is
/// released one level too shallow and the outermost one is never released
/// at all.
#[test]
fn test_return_from_a_nested_arm_keeps_outer_scopes_balanced() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn key_for(index int) String?
    Some(f"key_{index}")

fn value_for(index int) int?
    Some(index)

fn collect() Map<String, int>?
    var entries = Map<String, int>()
    match key_for(0)
        Some(key)
            match value_for(7)
                Some(value)
                    entries.set(key, value)
                None
                    return None
        None
            return None
    Some(entries)

fn main()
    match collect()
        Some(entries): println(f"{entries.length()}")
        None: println("none")
"#,
        "1",
    );
}

/// The same shape driven by a loop: the imbalance compounds per iteration, so
/// what leaks one allocation once leaks one per pass here.
#[test]
fn test_return_from_a_nested_arm_in_a_loop_stays_balanced() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn key_for(index int) String?
    Some(f"key_{index}")

fn value_for(index int) int?
    Some(index)

fn collect(count int) Map<String, int>?
    var entries = Map<String, int>()
    var index = 0
    while index < count
        match key_for(index)
            Some(key)
                match value_for(index)
                    Some(value)
                        entries.set(key, value)
                    None
                        return None
            None
                return None
        index = index + 1
    Some(entries)

fn main()
    match collect(50)
        Some(entries): println(f"{entries.length()}")
        None: println("none")
"#,
        "50",
    );
}

/// An arm that exits with `return` leaves the match through a path the join
/// block never sees, so the subject the match copied to dispatch on has to be
/// released on the way out as well — otherwise the value being matched on
/// outlives every reference to it.
#[test]
fn test_return_from_an_arm_releases_the_match_subject() {
    assert_runs_with_output(
        r#"
enum Tag
    Named(String)
    Empty

fn label(tag Tag) String?
    match tag
        Tag.Named(text)
            return None
        default: None

fn main()
    let tag = Tag.Named("a" + "b")
    match label(tag)
        Some(text): println(text)
        None: println("none")
"#,
        "none",
    );
}

/// The subject that leaks here is the *call result* the match dispatches on:
/// nothing but the match owns it, so an arm leaving early strands the whole
/// value, payload included.
#[test]
fn test_return_from_an_arm_releases_a_call_result_subject() {
    assert_runs_with_output(
        r#"
enum Tag
    Named(String)
    Empty

fn tagged(text String) Tag: Tag.Named(text + "!")

fn label(seed String) String?
    match tagged(seed)
        Tag.Named(text)
            return None
        default: None

fn main()
    match label("a" + "b")
        Some(text): println(text)
        None: println("none")
"#,
        "none",
    );
}

/// `break` leaves the match the same way `return` does, and a loop makes the
/// imbalance visible only if the subject is released once per pass.
#[test]
fn test_break_from_an_arm_releases_the_match_subject() {
    assert_runs_with_output(
        r#"
enum Tag
    Named(String)
    Empty

fn tagged(index int) Tag: Tag.Named(f"tag_{index}")

fn main()
    var index = 0
    var seen = 0
    while index < 50
        match tagged(index)
            Tag.Named(text)
                seen = seen + 1
                if seen == 50
                    break
            default: seen = seen
        index = index + 1
    println(f"{seen}")
"#,
        "50",
    );
}
