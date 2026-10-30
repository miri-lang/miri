// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the pass that drops a uniqueness check a dominating write already
//! answered.
//!
//! Each program writes through a path more than once. The checks that survive
//! are counted in the blocks still reachable from the entry, after the whole
//! pipeline lowered, counted references and elided what it could, so each test
//! pins how many times the compiled body asks whether a value is shared.

use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::analysis::dominators::DominatorTree;
use miri::mir::analysis::loops::natural_loops;
use miri::mir::body::DeviceHandleId;
use miri::mir::optimization::elide_unshare_checks;
use miri::mir::optimization::loop_unsharing::hoist_loop_unsharing;
use miri::mir::{
    AggregateKind, ArgMode, BasicBlock, BasicBlockData, Body, Discriminant, ExecutionModel,
    GpuLaunchArgs, Local, LocalDecl, Operand, Place, PlaceElem, Rvalue, Statement, StatementKind,
    Terminator, TerminatorKind,
};
use miri::pipeline::Pipeline;

use crate::integration::memory::owned_parameters::nbody_owned;
use miri::runtime_fns::rt;

const CELL: &str = r#"
use system.io
use system.collections.list

struct Cell
    x int
    name String
"#;

/// The bodies `source` compiles to, after reference counting and elision.
fn bodies_of(source: &str) -> Vec<(String, Body)> {
    Pipeline::new()
        .get_mir_bodies_with_rc(source)
        .expect("pipeline should lower the program")
}

/// The body `source` compiles to whose link name ends in `suffix`.
fn body_named(source: &str, suffix: &str) -> Body {
    bodies_of(source)
        .into_iter()
        .find(|(name, _)| name.ends_with(suffix))
        .map(|(_, body)| body)
        .unwrap_or_else(|| panic!("no body named *{suffix}"))
}

/// The number of uniqueness checks the body whose link name ends in `suffix`
/// still reaches.
fn checks_in(source: &str, suffix: &str) -> usize {
    reachable_checks(&body_named(source, suffix))
}

fn reachable_blocks(body: &Body) -> impl Iterator<Item = &BasicBlockData> {
    let unreachable = body.find_unreachable_blocks();
    body.basic_blocks
        .iter()
        .enumerate()
        .filter(move |(index, _)| !unreachable.contains(index))
        .map(|(_, block)| block)
}

fn calls_runtime(terminator: Option<&Terminator>, name: &str) -> bool {
    matches!(
        terminator.map(|t| &t.kind),
        Some(TerminatorKind::Call { func, .. }) if func.called_runtime_name() == Some(name)
    )
}

fn reachable_checks(body: &Body) -> usize {
    reachable_blocks(body)
        .filter(|block| calls_runtime(block.terminator.as_ref(), rt::RC_IS_SHARED))
        .count()
}

/// The copy paths `body` still reaches: a list cloned, or a struct rebuilt
/// from the element a write goes through.
fn reachable_copies(body: &Body) -> usize {
    let clones = reachable_blocks(body)
        .filter(|block| calls_runtime(block.terminator.as_ref(), rt::LIST_CLONE))
        .count();
    let rebuilt = reachable_blocks(body)
        .flat_map(|block| &block.statements)
        .filter(|statement| {
            matches!(
                &statement.kind,
                StatementKind::Assign(_, Rvalue::Aggregate(AggregateKind::Struct(_), _))
            )
        })
        .count();
    clones + rebuilt
}

fn with_cell(function: &str) -> String {
    format!("{CELL}\n{function}\n\nfn main()\n    println(\"ok\")\n")
}

/// The copy-on-write calls `body` still reaches.
fn reachable_copy_on_writes(body: &Body) -> usize {
    reachable_blocks(body)
        .filter(|block| calls_runtime(block.terminator.as_ref(), rt::LIST_COW))
        .count()
}

#[test]
fn test_a_second_write_to_a_local_list_does_not_ask_to_copy_it_again() {
    let source = with_cell(
        r#"
fn touch(n int) int
    var l = List([1, 2, 3])
    l[0] = n
    l[1] = n
    l[2] = n
    l[0] + l[1] + l[2]
"#,
    );
    let body = body_named(&source, ".touch");
    assert_eq!(reachable_copy_on_writes(&body), 1);
}

#[test]
fn test_binding_the_list_between_writes_keeps_its_copy_on_write() {
    let source = with_cell(
        r#"
fn touch(n int) int
    var l = List([1, 2, 3])
    l[0] = n
    let s = l
    l[1] = n
    s[1] + l[1]
"#,
    );
    let body = body_named(&source, ".touch");
    assert_eq!(reachable_copy_on_writes(&body), 2);
}

#[test]
fn test_a_write_in_a_loop_keeps_its_copy_on_write() {
    // Every iteration but the first may follow a binding of the list, so the
    // write in the loop asks each time; the write after the loop, which
    // nothing between shares, does not ask again.
    let source = with_cell(
        r#"
fn touch(n int) int
    var l = List([1, 2, 3])
    l[0] = n
    var i = 0
    while i < n
        l[1] = i
        let s = l
        i = i + s[0]
    l[2] = n
    l[2] = 0
    l[0] + l[1]
"#,
    );
    let body = body_named(&source, ".touch");
    assert_eq!(reachable_copy_on_writes(&body), 3);
}

#[test]
fn test_a_list_bound_from_a_parameter_asks_to_copy_once() {
    // The caller may still hold the parameter's list, so the first write
    // through the local bound to it asks; that answer makes the local's list
    // its own, so the second write does not ask again.
    let source = with_cell(
        r#"
fn touch(l [int], n int) int
    var m = l
    m[0] = n
    m[1] = n
    m[0] + m[1]
"#,
    );
    let body = body_named(&source, ".touch");
    assert_eq!(reachable_copy_on_writes(&body), 1);
}

#[test]
fn test_writes_into_maps_sets_and_arrays_keep_value_semantics() {
    crate::integration::utils::assert_runs_with_output(
        r#"
use system.io
use system.collections.list
use system.collections.map
use system.collections.set

fn main()
    var m = {"a": 1}
    m["b"] = 2
    let mm = m
    m["a"] = 3
    m["c"] = 4
    var s = {1, 2}
    s.add(3)
    let ss = s
    s.add(4)
    var a = [1, 2, 3]
    a[0] = 5
    let aa = a
    a[1] = 6
    a[2] = 7
    println(f"{mm['a']} {mm.length()} {m['a']} {m.length()} {ss.length()} {s.length()} {aa[1]} {a[1]} {a[2]}")
"#,
        "1 2 3 3 3 4 2 6 7",
    );
}

#[test]
fn test_a_write_after_binding_the_list_still_copies_it() {
    crate::integration::utils::assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List([1, 2, 3])
    l[0] = 7
    l[2] = 8
    let s = l
    l[1] = 9
    l[2] = 10
    println(f"{s[0]} {s[1]} {s[2]} {l[0]} {l[1]} {l[2]}")
"#,
        "7 2 8 7 9 10",
    );
}

/// Whether `block` lies on a cycle of `body`'s control flow.
fn is_on_a_cycle(body: &Body, block: usize) -> bool {
    let successors = |index: usize| {
        body.basic_blocks
            .get(index)
            .and_then(|data| data.terminator.as_ref())
            .map(|terminator| terminator.successors())
            .unwrap_or_default()
    };
    let mut seen = vec![false; body.basic_blocks.len()];
    let mut stack: Vec<usize> = successors(block).iter().map(|b| b.0).collect();
    while let Some(next) = stack.pop() {
        if next == block {
            return true;
        }
        if seen.get(next).copied().unwrap_or(true) {
            continue;
        }
        seen[next] = true;
        stack.extend(successors(next).iter().map(|b| b.0));
    }
    false
}

/// The reachable blocks of `body` that call runtime function `name` from
/// inside a loop.
fn calls_in_loops(body: &Body, name: &str) -> usize {
    let unreachable = body.find_unreachable_blocks();
    (0..body.basic_blocks.len())
        .filter(|index| !unreachable.contains(index))
        .filter(|index| calls_runtime(body.basic_blocks[*index].terminator.as_ref(), name))
        .filter(|index| is_on_a_cycle(body, *index))
        .count()
}

#[test]
fn test_a_list_written_every_iteration_is_made_its_own_before_the_loop() {
    let source = with_cell(
        r#"
fn mark(n int) int
    var flags = List([false, false, false, false])
    var m = 0
    while m < n
        flags[m % 4] = true
        m = m + 1
    if flags[0]: 1 else: 0
"#,
    );
    let body = body_named(&source, ".mark");
    assert_eq!(calls_in_loops(&body, rt::LIST_COW), 0);
    assert_eq!(reachable_copy_on_writes(&body), 1);
}

#[test]
fn test_a_list_path_written_every_iteration_makes_the_list_its_own_before_the_loop() {
    let source = with_cell(
        r#"
fn touch(start [Cell], n int) [Cell]
    var l = start
    var i = 0
    while i < n
        l[0].x = i
        i = i + 1
    l
"#,
    );
    let body = body_named(&source, ".touch");
    // The loop calls nothing else, so the list is made its own once before
    // it; the element is still asked about in the loop.
    assert_eq!(calls_in_loops(&body, rt::RC_IS_SHARED), 1);
    assert_eq!(calls_in_loops(&body, rt::LIST_COW), 0);
    assert_eq!(reachable_copy_on_writes(&body), 1);
}

#[test]
fn test_a_list_path_written_in_a_loop_that_calls_out_is_checked_in_the_loop() {
    let source = with_cell(
        r#"
fn report(i int)
    println(f"{i}")

fn touch(start [Cell], n int) [Cell]
    var l = start
    var i = 0
    while i < n
        l[0].x = i
        report(i)
        i = i + 1
    l
"#,
    );
    let body = body_named(&source, ".touch");
    // A call can change the list's header, so code generation reloads it at
    // every access anyway; the list stays a value of each iteration's own
    // rather than one held live across the call.
    assert_eq!(calls_in_loops(&body, rt::RC_IS_SHARED), 2);
    assert_eq!(reachable_copy_on_writes(&body), 0);
}

#[test]
fn test_a_list_shared_in_the_loop_is_checked_every_iteration() {
    let source = with_cell(
        r#"
fn touch(start [Cell], n int) int
    var l = start
    var i = 0
    var total = 0
    while i < n
        l[0].x = i
        let s = l
        total = total + s[0].x
        i = i + 1
    total
"#,
    );
    let body = body_named(&source, ".touch");
    assert_eq!(calls_in_loops(&body, rt::RC_IS_SHARED), 2);
}

#[test]
fn test_a_list_written_on_some_iterations_only_is_not_copied_before_the_loop() {
    // Copying before the loop would copy a shared list the loop might never
    // write, so the check stays where the write is.
    let source = with_cell(
        r#"
fn mark(n int) int
    var flags = List([false, false, false, false])
    var m = 0
    while m < n
        if m == 3
            flags[0] = true
        m = m + 1
    if flags[0]: 1 else: 0
"#,
    );
    let body = body_named(&source, ".mark");
    assert_eq!(calls_in_loops(&body, rt::LIST_COW), 1);
}

/// For each copy-on-write call `body` still reaches, how many loops hold it:
/// 0 before every loop, 1 inside one loop but before the loop it holds, and
/// so on.
fn copy_on_write_loop_depths(body: &Body) -> Vec<usize> {
    let dominators = DominatorTree::compute(body);
    let loops = natural_loops(body, &dominators);
    let unreachable = body.find_unreachable_blocks();
    (0..body.basic_blocks.len())
        .filter(|index| !unreachable.contains(index))
        .filter(|index| calls_runtime(body.basic_blocks[*index].terminator.as_ref(), rt::LIST_COW))
        .map(|index| {
            loops
                .iter()
                .filter(|found| found.blocks.contains(&index))
                .count()
        })
        .collect()
}

#[test]
fn test_a_list_written_in_every_iteration_of_an_inner_loop_is_made_its_own_before_the_outer_one() {
    let source = with_cell(
        r#"
fn spread(n int) int
    var counts = List([0, 0, 0, 0])
    var i = 0
    while i < n
        var j = 0
        while j < n
            counts[j % 4] = counts[j % 4] + i
            j = j + 1
        i = i + 1
    counts[0]
"#,
    );
    let body = body_named(&source, ".spread");
    assert_eq!(calls_in_loops(&body, rt::LIST_COW), 0, "{body}");
    assert_eq!(reachable_copy_on_writes(&body), 1, "{body}");
}

#[test]
fn test_a_list_path_written_in_every_iteration_of_an_inner_loop_is_made_its_own_before_the_outer_one(
) {
    let source = with_cell(
        r#"
fn touch(start [Cell], n int) [Cell]
    var l = start
    var i = 0
    while i < n
        var j = 0
        while j < n
            l[j % 2].x = l[j % 2].x + i
            j = j + 1
        i = i + 1
    l
"#,
    );
    let body = body_named(&source, ".touch");
    assert_eq!(calls_in_loops(&body, rt::LIST_COW), 0, "{body}");
    assert_eq!(reachable_copy_on_writes(&body), 1, "{body}");
}

#[test]
fn test_a_loop_after_a_nest_writing_the_list_asks_nothing_more() {
    let source = with_cell(
        r#"
fn spread(n int) int
    var counts = List([0, 0, 0, 0])
    var i = 0
    while i < n
        var j = 0
        while j < n
            counts[j % 4] = counts[j % 4] + i
            j = j + 1
        i = i + 1
    var k = 0
    while k < 4
        counts[k] = counts[k] * 2
        k = k + 1
    counts[0]
"#,
    );
    let body = body_named(&source, ".spread");
    assert_eq!(calls_in_loops(&body, rt::LIST_COW), 0, "{body}");
    assert_eq!(reachable_copy_on_writes(&body), 1, "{body}");
}

#[test]
fn test_an_outer_loop_sharing_the_list_asks_before_each_run_of_the_inner_one() {
    let source = with_cell(
        r#"
fn spread(n int) int
    var counts = List([0, 0, 0, 0])
    var total = 0
    var i = 0
    while i < n
        let snapshot = counts
        var j = 0
        while j < n
            counts[j % 4] = snapshot[j % 4] + i
            j = j + 1
        total = total + snapshot[0]
        i = i + 1
    total + counts[0]
"#,
    );
    let body = body_named(&source, ".spread");
    assert_eq!(copy_on_write_loop_depths(&body), vec![1], "{body}");
}

#[test]
fn test_an_outer_loop_rebinding_the_list_asks_before_each_run_of_the_inner_one() {
    let source = with_cell(
        r#"
fn spread(start [int], n int) int
    var counts = List([0, 0, 0, 0])
    var i = 0
    while i < n
        counts = start
        var j = 0
        while j < n
            counts[j % 4] = counts[j % 4] + i
            j = j + 1
        i = i + 1
    counts[0]
"#,
    );
    let body = body_named(&source, ".spread");
    assert_eq!(copy_on_write_loop_depths(&body), vec![1], "{body}");
}

#[test]
fn test_a_snapshot_taken_inside_a_loop_keeps_its_values() {
    crate::integration::utils::assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var l = List([0, 0, 0])
    let before = l
    var snaps = List<[int]>()
    var i = 0
    while i < 3
        l[i] = i + 1
        snaps.push(l)
        i = i + 1
    println(f"{before[0]} {snaps[0][0]} {snaps[0][1]} {snaps[1][1]} {snaps[1][2]} {l[2]}")
"#,
        "0 1 0 2 0 3",
    );
}

#[test]
fn test_a_list_built_empty_and_pushed_to_in_a_loop_asks_nothing() {
    // A constructor hands back a list nothing else holds, and a push keeps
    // nothing it writes, so no push in the loop asks to copy the list.
    let source = with_cell(
        r#"
fn times(v [float]) [float]
    let n = v.length()
    var out = List<float>()
    var i = 0
    while i < n
        var sum = 0.0
        var j = 0
        while j < n
            sum = sum + v[j]
            j += 1
        out.push(sum)
        i += 1
    out
"#,
    );
    let body = body_named(&source, ".times");
    assert_eq!(reachable_copy_on_writes(&body), 0, "{body}");
}

#[test]
fn test_a_push_after_binding_the_list_in_the_loop_still_asks() {
    let source = with_cell(
        r#"
fn collect(n int) int
    var out = List<int>()
    var total = 0
    var i = 0
    while i < n
        out.push(i)
        let seen = out
        total += seen.length()
        i += 1
    total
"#,
    );
    let body = body_named(&source, ".collect");
    assert_eq!(calls_in_loops(&body, rt::LIST_COW), 1, "{body}");
}

#[test]
fn test_a_push_after_handing_the_list_to_a_function_still_asks() {
    let source = with_cell(
        r#"
fn keep(into [[int]], l [int]) [[int]]
    var kept = into
    kept.push(l)
    kept

fn collect(n int) int
    var out = List<int>()
    var snaps = List<[int]>()
    out.push(n)
    snaps = keep(snaps, out)
    out.push(n)
    snaps.length() + out.length()
"#,
    );
    let body = body_named(&source, ".collect");
    assert_eq!(reachable_copy_on_writes(&body), 1, "{body}");
}

#[test]
fn test_pushes_after_a_binding_or_a_call_keep_value_semantics() {
    crate::integration::utils::assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn keep(into [[int]], l [int]) [[int]]
    var kept = into
    kept.push(l)
    kept

fn main()
    var a = List<int>()
    a.push(1)
    let b = a
    a.push(2)
    var snaps = List<[int]>()
    var i = 0
    while i < 3
        a.push(i)
        snaps = keep(snaps, a)
        i += 1
    println(f"{b.length()} {a.length()} {snaps[0].length()} {snaps[1].length()} {snaps[2].length()}")
"#,
        "1 5 3 4 5",
    );
}

#[test]
fn test_an_owned_parameter_written_twice_checks_each_place_once() {
    // A parameter cannot be written through, so a body that writes its
    // elements binds it first; an owned one is moved into the binding, whose
    // checks then answer the repeated ones as any local's do.
    let source = format!(
        "{CELL}\n{}",
        r#"
fn touch(start [Cell], i int) [Cell]
    var l = start
    l[i].x = 1
    l[i].x = 2
    l

fn main()
    var c = List([Cell(x: 0, name: "a")])
    c = touch(c, 0)
    println(f"{c[0].x}")
"#
    );
    let body = body_named(&source, ".touch");
    assert!(
        body.is_owned_param(Local(1)),
        "the list is handed over to touch"
    );
    assert_eq!(reachable_checks(&body), 2);
}

#[test]
fn test_a_borrowed_parameter_written_twice_checks_each_place_once_and_copies() {
    // The caller keeps a borrowed list, so the binding shares it: its first
    // check copies, and the copy answers the second write's checks.
    let source = with_cell(
        r#"
fn touch(start [Cell], i int) int
    var l = start
    l[i].x = 1
    l[i].x = 2
    start[i].x + l[i].x
"#,
    );
    let body = body_named(&source, ".touch");
    assert!(
        !body.is_owned_param(Local(1)),
        "touch reads the list after the writes"
    );
    assert_eq!(reachable_checks(&body), 2);
    crate::integration::utils::assert_runs_with_output(
        r#"
use system.io
use system.collections.list

struct Cell
    x int
    name String

fn touch(start [Cell], i int) int
    var l = start
    l[i].x = 5
    l[i].x = 7
    start[i].x * 10 + l[i].x

fn main()
    let c = List([Cell(x: 1, name: "a")])
    println(f"{touch(c, 0)} {c[0].x}")
"#,
        "17 1",
    );
}

#[test]
fn test_repeated_writes_to_one_element_check_the_path_once() {
    let source = with_cell(
        r#"
fn touch(start [Cell], i int) [Cell]
    var l = start
    l[i].x = 1
    l[i].x = 2
    l[i].name = "y"
    l
"#,
    );
    let body = body_named(&source, ".touch");
    assert_eq!(reachable_checks(&body), 2);
    // Each removed check jumps to its "not shared" path, so only the copy
    // paths of the two checks kept are still reached.
    assert_eq!(reachable_copies(&body), 2);
}

#[test]
fn test_binding_the_list_between_writes_keeps_every_check() {
    let source = with_cell(
        r#"
fn touch(start [Cell], i int) int
    var l = start
    l[i].x = 1
    let s = l
    l[i].x = 2
    s[i].x + l[i].x
"#,
    );
    assert_eq!(checks_in(&source, ".touch"), 4);
}

#[test]
fn test_binding_an_element_between_writes_keeps_the_element_check() {
    let source = with_cell(
        r#"
fn touch(start [Cell], i int, j int) int
    var l = start
    l[i].x = 1
    let e = l[j]
    l[i].x = 2
    e.x + l[i].x
"#,
    );
    assert_eq!(checks_in(&source, ".touch"), 3);
}

#[test]
fn test_changing_the_index_between_writes_keeps_the_element_check() {
    let source = with_cell(
        r#"
fn touch(start [Cell], j int) [Cell]
    var l = start
    var i = 0
    l[i].x = 1
    i = j
    l[i].x = 2
    l
"#,
    );
    assert_eq!(checks_in(&source, ".touch"), 3);
}

#[test]
fn test_writes_through_one_computed_index_check_the_element_once() {
    // Each write computes `i + 1` into a temp of its own; the two temps hold
    // one value, so the second write reaches the element the first checked.
    let source = with_cell(
        r#"
fn touch(start [Cell], i int) [Cell]
    var l = start
    l[i + 1].x = 1
    l[i + 1].name = "y"
    l
"#,
    );
    assert_eq!(checks_in(&source, ".touch"), 2);
}

#[test]
fn test_changing_a_computed_index_between_writes_keeps_the_element_check() {
    let source = with_cell(
        r#"
fn touch(start [Cell], j int) [Cell]
    var l = start
    var i = j
    l[i + 1].x = 1
    i = j + 1
    l[i + 1].x = 2
    l
"#,
    );
    assert_eq!(checks_in(&source, ".touch"), 3);
}

#[test]
fn test_a_computed_index_in_a_loop_is_checked_once_per_iteration() {
    // Within an iteration the second write reaches the element the first
    // checked; the index changes every iteration, so the element check stays
    // in the loop. The list is made its own once before it. The loop reads
    // `l[i + 1]` below a parameter, so it is versioned: the check is counted
    // once in each of its two copies.
    let source = with_cell(
        r#"
fn touch(start [Cell], n int) [Cell]
    var l = start
    var i = 0
    while i < n
        l[i + 1].x = i
        l[i + 1].x = i + 1
        i = i + 1
    l
"#,
    );
    let body = body_named(&source, ".touch");
    assert_eq!(reachable_checks(&body), 2);
    assert_eq!(calls_in_loops(&body, rt::RC_IS_SHARED), 2);
    assert_eq!(reachable_copy_on_writes(&body), 1);
}

#[test]
fn test_writes_through_one_computed_index_keep_value_semantics() {
    crate::integration::utils::assert_runs_with_output(
        r#"
use system.io
use system.collections.list

struct Cell
    x int
    y int

fn main()
    var l = List([Cell(x: 0, y: 0), Cell(x: 0, y: 0), Cell(x: 0, y: 0)])
    let kept = l
    var i = 0
    l[i + 1].x = 1
    l[i + 1].y = 2
    i = 1
    l[i + 1].x = 3
    println(f"{kept[1].x} {kept[1].y} {l[1].x} {l[1].y} {l[2].x} {l[2].y}")
"#,
        "0 0 1 2 3 0",
    );
}

#[test]
fn test_passing_the_list_to_a_call_between_writes_keeps_every_check() {
    let source = with_cell(
        r#"
fn total(l [Cell]) int
    l[0].x

fn touch(start [Cell], i int) int
    var l = start
    l[i].x = 1
    let t = total(l)
    l[i].x = 2
    t + l[i].x
"#,
    );
    assert_eq!(checks_in(&source, ".touch"), 4);
}

#[test]
fn test_capturing_the_list_between_writes_keeps_every_check() {
    let source = with_cell(
        r#"
fn touch(start [Cell], i int) int
    var l = start
    l[i].x = 1
    let f = fn() int: l[0].x
    l[i].x = 2
    f() + l[i].x
"#,
    );
    assert_eq!(checks_in(&source, ".touch"), 4);
}

#[test]
fn test_a_path_through_a_class_field_keeps_every_check() {
    let source = with_cell(
        r#"
class Holder
    var items [Cell]

    fn touch(i int)
        self.items[i].x = 1
        self.items[i].x = 2
"#,
    );
    assert_eq!(checks_in(&source, "Holder.touch"), 4);
}

#[test]
fn test_a_branch_sharing_the_list_keeps_the_check_after_the_join() {
    let source = with_cell(
        r#"
fn touch(start [Cell], i int, keep bool) int
    var l = start
    var held = List<Cell>()
    l[i].x = 1
    if keep
        held = l
    else
        held = List<Cell>()
    l[i].x = 2
    held.length() + l[i].x
"#,
    );
    assert_eq!(checks_in(&source, ".touch"), 4);
}

#[test]
fn test_nbody_advance_makes_the_bodies_its_own_once_before_its_loops() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/bench/workloads/nbody/nbody.mi"
    );
    let source = std::fs::read_to_string(path).expect("nbody source should be readable");
    let body = body_named(&source, ".advance");
    assert_eq!(reachable_checks(&body), 0);
    assert_eq!(calls_in_loops(&body, rt::LIST_COW), 0, "{body}");
    assert_eq!(reachable_copy_on_writes(&body), 1, "{body}");
}

/// The n-body step whose allocation count and checksum
/// `integration::memory::owned_parameters` pins writes six paths, and lowering
/// asks before each whether the list is shared; a body in it is held as its
/// bytes, never shared, and never asked about. Each loop calls nothing else,
/// so the list is made its own once before it and no write asks at all; that
/// checksum guards every write the pass lets run without asking.
#[test]
fn test_the_pinned_nbody_step_asks_fewer_times_than_it_writes() {
    assert_eq!(checks_in(&nbody_owned(1000), ".advance"), 0);
}

/// The pass recognizes a check only in the shape lowering emits: the call
/// ends its block and continues at a block entered from it alone that does
/// nothing but branch on the answer, 0 meaning "not shared". Every check a
/// program keeps still has that shape, so a lowering change breaking it fails
/// here instead of silently turning the pass off.
#[test]
fn test_every_check_lowering_emits_ends_its_block_and_branches_alone() {
    let nbody = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/bench/workloads/nbody/nbody.mi"
    ))
    .expect("nbody source should be readable");
    let mut seen = 0;
    for source in [nbody, nbody_owned(10)] {
        for (name, body) in bodies_of(&source) {
            seen += assert_checks_branch_alone(&name, &body);
        }
    }
    assert!(seen > 0, "no check left to inspect");
}

/// Asserts every reachable check in `body` has lowering's shape; returns how
/// many there are.
fn assert_checks_branch_alone(name: &str, body: &Body) -> usize {
    let predecessors = body.predecessor_counts();
    let mut checks = 0;
    for block in reachable_blocks(body) {
        let Some(TerminatorKind::Call {
            func,
            destination,
            target,
            ..
        }) = block.terminator.as_ref().map(|t| &t.kind)
        else {
            continue;
        };
        if func.called_runtime_name() != Some(rt::RC_IS_SHARED) {
            continue;
        }
        checks += 1;
        let switch = target.unwrap_or_else(|| panic!("{name}: a check never returns"));
        let switch_block = &body.basic_blocks[switch.0];
        assert!(
            switch_block.statements.is_empty(),
            "{name}: bb{} does work",
            switch.0
        );
        assert_eq!(
            predecessors[switch.0], 1,
            "{name}: bb{} is shared",
            switch.0
        );
        let branches_on_answer = matches!(
            switch_block.terminator.as_ref().map(|t| &t.kind),
            Some(TerminatorKind::SwitchInt { discr: Operand::Copy(answer), targets, .. })
                if answer == destination
                    && targets.len() == 1
                    && targets[0].0 == Discriminant::from(0)
        );
        assert!(
            branches_on_answer,
            "{name}: bb{} branches elsewhere",
            switch.0
        );
    }
    checks
}

fn span() -> Span {
    Span::default()
}

fn int() -> Type {
    Type::new(TypeKind::Int, span())
}

fn local(index: usize) -> Place {
    Place::new(Local(index))
}

fn projected(base: usize, elem: PlaceElem) -> Place {
    Place {
        local: Local(base),
        projection: vec![elem],
    }
}

fn element(base: usize, index: usize) -> Place {
    projected(base, PlaceElem::Index(Local(index)))
}

fn assign(place: Place, rvalue: Rvalue) -> StatementKind {
    StatementKind::Assign(place, rvalue)
}

fn built_struct() -> Rvalue {
    Rvalue::Aggregate(AggregateKind::Struct(int()), Vec::new())
}

fn runtime_call(
    name: &str,
    args: Vec<Operand>,
    destination: Place,
    target: BasicBlock,
) -> TerminatorKind {
    TerminatorKind::Call {
        func: Operand::runtime(name, span()),
        args,
        arg_modes: Vec::new(),
        arg_handles: Vec::new(),
        destination,
        target: Some(target),
    }
}

fn terminator(body: &Body, block: usize) -> Option<&TerminatorKind> {
    body.basic_blocks[block]
        .terminator
        .as_ref()
        .map(|t| &t.kind)
}

/// The checks `body` still reaches once the pass has run over it.
fn checks_left(mut body: Body) -> usize {
    elide_unshare_checks(&mut body);
    reachable_checks(&body)
}

/// Where a check built by [`Builder::check`] sits: the block asking, and the
/// block the answer "not shared" continues at.
struct CheckSite {
    call: usize,
    unshared: usize,
}

/// How many int locals a built body declares, `_0` and the parameter `_1`
/// included, before the answer locals its checks add.
const LOCALS: usize = 12;

/// Builds a host body taking one parameter, `_1`, block by block: statements
/// gather in the open block, and each terminator closes it and opens the next.
struct Builder {
    body: Body,
    open: Vec<StatementKind>,
}

impl Builder {
    fn new() -> Self {
        let mut body = Body::new(1, span(), ExecutionModel::Cpu);
        for _ in 0..LOCALS {
            body.new_local(LocalDecl::new(int(), span()));
        }
        Self {
            body,
            open: Vec::new(),
        }
    }

    fn statements(&mut self, kinds: Vec<StatementKind>) -> &mut Self {
        self.open.extend(kinds);
        self
    }

    /// The index of the open block.
    fn current(&self) -> usize {
        self.body.basic_blocks.len()
    }

    fn close(&mut self, kind: TerminatorKind) {
        let statements = std::mem::take(&mut self.open)
            .into_iter()
            .map(|kind| Statement { kind, span: span() })
            .collect();
        self.body.basic_blocks.push(BasicBlockData {
            statements,
            terminator: Some(Terminator::new(kind, span())),
            is_cleanup: false,
        });
    }

    /// Closes the open block with the terminator `kind` builds from the block
    /// it continues at.
    fn call(&mut self, kind: impl FnOnce(BasicBlock) -> TerminatorKind) -> &mut Self {
        let next = BasicBlock(self.current() + 1);
        self.close(kind(next));
        self
    }

    /// `destination = list_clone(_11)`: a value only `destination` holds.
    fn fresh(&mut self, destination: Place) -> &mut Self {
        let source = vec![Operand::Copy(local(11))];
        self.call(|next| runtime_call(rt::LIST_CLONE, source, destination, next))
    }

    /// Asks whether `place` is shared, giving it a struct just built when it
    /// is, as lowering's copy path gives it a copy only it holds.
    fn check(&mut self, place: Place) -> CheckSite {
        let copy = vec![assign(place.clone(), built_struct())];
        self.check_with_copy(place, copy)
    }

    /// Asks whether `place` is shared in the shape lowering emits: the call,
    /// a block branching on its answer alone, a copy path running `copy`, and
    /// the join both paths continue at.
    fn check_with_copy(&mut self, place: Place, copy: Vec<StatementKind>) -> CheckSite {
        let answer = self.body.new_local(LocalDecl::new(int(), span()));
        let call = self.current();
        let (copy_block, join) = (BasicBlock(call + 2), BasicBlock(call + 3));
        let asked = vec![Operand::Copy(place)];
        self.call(|next| runtime_call(rt::RC_IS_SHARED, asked, Place::new(answer), next));
        self.close(TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(answer)),
            targets: vec![(Discriminant::from(0), join)],
            otherwise: copy_block,
        });
        self.open = copy;
        self.close(TerminatorKind::Goto { target: join });
        CheckSite {
            call,
            unshared: join.0,
        }
    }

    /// Branches on the parameter into two arms running `left` and `right`,
    /// which meet in the block left open.
    fn diamond(&mut self, left: Vec<StatementKind>, right: Vec<StatementKind>) -> &mut Self {
        let branch = self.current();
        let join = BasicBlock(branch + 3);
        self.close(TerminatorKind::SwitchInt {
            discr: Operand::Copy(local(1)),
            targets: vec![(Discriminant::from(0), BasicBlock(branch + 1))],
            otherwise: BasicBlock(branch + 2),
        });
        for arm in [left, right] {
            self.open = arm;
            self.close(TerminatorKind::Goto { target: join });
        }
        self
    }

    fn finish(mut self) -> Body {
        self.close(TerminatorKind::Return);
        self.body
    }
}

/// A fresh clone in `_2`, then `between`, then a check on `_2`.
fn fresh_then_check(between: Vec<StatementKind>) -> Builder {
    let mut builder = Builder::new();
    builder.fresh(local(2)).statements(between);
    builder.check(local(2));
    builder
}

#[test]
fn test_a_fresh_clone_answers_the_check_on_it_with_a_jump_to_its_no_path() {
    let mut builder = Builder::new();
    builder.fresh(local(2));
    let site = builder.check(local(2));
    let mut body = builder.finish();
    elide_unshare_checks(&mut body);
    assert_eq!(reachable_checks(&body), 0);
    assert_eq!(
        terminator(&body, site.call),
        Some(&TerminatorKind::Goto {
            target: BasicBlock(site.unshared)
        })
    );
    for cut_off in [site.call + 1, site.call + 2] {
        assert_eq!(
            terminator(&body, cut_off),
            Some(&TerminatorKind::Unreachable)
        );
        assert!(body.basic_blocks[cut_off].statements.is_empty());
    }
}

#[test]
fn test_a_second_check_on_one_element_is_answered_by_the_first() {
    let mut builder = Builder::new();
    builder.fresh(local(2));
    builder.check(element(2, 6));
    builder.check(element(2, 6));
    assert_eq!(checks_left(builder.finish()), 1);
}

/// `_2 = move _3` out of a fresh clone in `_3`, then `between`, then a check
/// on `_2`: the body a move-sourced fact is decided on.
fn moved_clone_then_check(between: Vec<StatementKind>) -> Body {
    let mut moved = vec![assign(local(2), Rvalue::Use(Operand::Move(local(3))))];
    moved.extend(between);
    let mut builder = Builder::new();
    builder.fresh(local(3)).statements(moved);
    builder.check(local(2));
    builder.finish()
}

#[test]
fn test_a_value_moved_out_of_a_fresh_clone_is_known_unshared() {
    assert_eq!(checks_left(moved_clone_then_check(Vec::new())), 0);
}

#[test]
fn test_a_moved_from_local_retained_afterwards_keeps_the_check() {
    let body = moved_clone_then_check(vec![StatementKind::IncRef(local(3))]);
    assert_eq!(checks_left(body), 1);
}

#[test]
fn test_a_moved_from_local_read_again_keeps_the_check() {
    let body = moved_clone_then_check(vec![assign(local(5), Rvalue::Len(local(3)))]);
    assert_eq!(checks_left(body), 1);
}

/// `_3.0` is known unshared — a struct just built, stored into a fresh clone —
/// and `_3` is read once, by the move; only the projection refuses the fact.
#[test]
fn test_a_value_moved_out_of_a_projected_place_keeps_the_check() {
    let mut builder = Builder::new();
    let field = projected(3, PlaceElem::Field(0));
    builder.fresh(local(3)).statements(vec![
        assign(field.clone(), built_struct()),
        assign(local(2), Rvalue::Use(Operand::Move(field))),
    ]);
    builder.check(local(2));
    assert_eq!(checks_left(builder.finish()), 1);
}

#[test]
fn test_a_value_moved_out_of_a_place_not_known_unshared_keeps_the_check() {
    let mut builder = Builder::new();
    builder.statements(vec![
        assign(local(3), Rvalue::Use(Operand::Copy(local(11)))),
        assign(local(2), Rvalue::Use(Operand::Move(local(3)))),
    ]);
    builder.check(local(2));
    assert_eq!(checks_left(builder.finish()), 1);
}

#[test]
fn test_a_struct_just_built_answers_the_check_on_it() {
    let mut builder = Builder::new();
    builder.statements(vec![assign(local(2), built_struct())]);
    builder.check(local(2));
    assert_eq!(checks_left(builder.finish()), 0);
}

#[test]
fn test_a_struct_just_built_and_aliased_keeps_the_check() {
    let mut builder = Builder::new();
    builder.statements(vec![
        assign(local(2), built_struct()),
        assign(local(3), Rvalue::Use(Operand::Copy(local(2)))),
    ]);
    builder.check(local(2));
    assert_eq!(checks_left(builder.finish()), 1);
}

#[test]
fn test_a_parameter_given_a_fresh_value_keeps_its_check() {
    let mut builder = Builder::new();
    builder.fresh(local(1));
    builder.check(local(1));
    assert_eq!(checks_left(builder.finish()), 1);
}

#[test]
fn test_a_local_whose_address_is_taken_keeps_its_check() {
    let mut builder = fresh_then_check(Vec::new());
    builder.statements(vec![assign(local(4), Rvalue::Ref(local(2)))]);
    assert_eq!(checks_left(builder.finish()), 1);
}

#[test]
fn test_an_element_indexed_by_a_local_whose_address_is_taken_keeps_its_check() {
    let mut builder = Builder::new();
    builder.fresh(local(2));
    builder.check(element(2, 6));
    builder.check(element(2, 6));
    builder.statements(vec![assign(local(7), Rvalue::Ref(local(6)))]);
    assert_eq!(checks_left(builder.finish()), 2);
}

#[test]
fn test_a_gpu_resident_binding_keeps_its_check() {
    let mut body = fresh_then_check(Vec::new()).finish();
    body.local_decls[2].device_handle = Some(DeviceHandleId(1));
    assert_eq!(checks_left(body), 1);
}

#[test]
fn test_a_captured_binding_keeps_its_check() {
    let mut body = fresh_then_check(Vec::new()).finish();
    body.env_capture_locals.push(Local(2));
    assert_eq!(checks_left(body), 1);
}

#[test]
fn test_a_gpu_body_is_left_alone() {
    let mut body = fresh_then_check(Vec::new()).finish();
    body.execution_model = ExecutionModel::GpuKernel;
    assert_eq!(checks_left(body), 1);
}

#[test]
fn test_a_release_or_the_end_of_storage_forgets_the_place() {
    for forget in [
        StatementKind::DecRef(local(2)),
        StatementKind::Dealloc(local(2)),
        StatementKind::StorageDead(local(2)),
    ] {
        let shown = format!("{forget:?}");
        let body = fresh_then_check(vec![forget]).finish();
        assert_eq!(checks_left(body), 1, "{shown} keeps the place unshared");
    }
}

#[test]
fn test_a_virtual_call_handed_the_place_forgets_it() {
    let mut builder = Builder::new();
    builder
        .fresh(local(2))
        .call(|next| TerminatorKind::VirtualCall {
            vtable_slot: 0,
            args: vec![Operand::Copy(local(2))],
            arg_modes: Vec::new(),
            param_types: Vec::new(),
            destination: local(4),
            target: Some(next),
        });
    builder.check(local(2));
    assert_eq!(checks_left(builder.finish()), 1);
}

#[test]
fn test_a_kernel_launch_handed_the_place_forgets_it() {
    let launch_args = GpuLaunchArgs::new(
        vec![Operand::Copy(local(2))],
        vec![None],
        vec![false],
        vec![false],
    )
    .expect("one capture with its metadata");
    let mut builder = Builder::new();
    builder
        .fresh(local(2))
        .call(|next| TerminatorKind::GpuLaunch {
            kernel: Operand::Copy(local(8)),
            grid: Operand::Copy(local(9)),
            block: Operand::Copy(local(10)),
            launch_args,
            scalar_args: Vec::new(),
            uniform_bound_x: None,
            uniform_bound_y: None,
            uniform_bound_z: None,
            uniform_start_x: None,
            uniform_start_y: None,
            uniform_start_z: None,
            destination: local(4),
            target: Some(next),
        });
    builder.check(local(2));
    assert_eq!(checks_left(builder.finish()), 1);
}

/// Two checks on `_2[_6]` around a call handed `_6` in `mode`.
fn element_checks_around_a_call(mode: ArgMode) -> Body {
    let mut builder = Builder::new();
    builder.fresh(local(2));
    builder.check(element(2, 6));
    builder.call(|next| TerminatorKind::Call {
        func: Operand::runtime("callee", span()),
        args: vec![Operand::Copy(local(6))],
        arg_modes: vec![mode],
        arg_handles: Vec::new(),
        destination: local(4),
        target: Some(next),
    });
    builder.check(element(2, 6));
    builder.finish()
}

#[test]
fn test_an_index_written_through_an_out_argument_forgets_the_element() {
    assert_eq!(checks_left(element_checks_around_a_call(ArgMode::Out)), 2);
    assert_eq!(
        checks_left(element_checks_around_a_call(ArgMode::Borrowed)),
        1
    );
}

#[test]
fn test_both_arms_keeping_the_place_unshared_answer_the_check_after_the_join() {
    let mut builder = Builder::new();
    builder.fresh(local(2)).diamond(Vec::new(), Vec::new());
    builder.check(local(2));
    assert_eq!(checks_left(builder.finish()), 0);
}

#[test]
fn test_one_arm_retaining_the_place_keeps_the_check_after_the_join() {
    let mut builder = Builder::new();
    builder
        .fresh(local(2))
        .diamond(Vec::new(), vec![StatementKind::IncRef(local(2))]);
    builder.check(local(2));
    assert_eq!(checks_left(builder.finish()), 1);
}

/// The first check is answered by the clone, so its copy path is dead: what
/// that path forgets never reaches the join, and the second check goes too.
#[test]
fn test_the_copy_path_of_an_answered_check_does_not_reach_the_join() {
    let mut builder = Builder::new();
    builder.fresh(local(2));
    builder.check_with_copy(local(2), vec![StatementKind::IncRef(local(2))]);
    builder.check(local(2));
    assert_eq!(checks_left(builder.finish()), 0);
}

/// Locals of [`nest_writing_a_list`].
const OUTER_INDEX: Local = Local(1);
const NEST_LIST: Local = Local(2);
const INNER_INDEX: Local = Local(3);
const OUTER_BELOW: Local = Local(4);
const INNER_BELOW: Local = Local(5);
const HANDED_BACK: Local = Local(6);
const REPLACEMENT: Local = Local(7);
/// Blocks of [`nest_writing_a_list`].
const OUTER_HEADER: usize = 1;
const OUTER_STEP: usize = 2;
const INNER_HEADER: usize = 3;
const INNER_ASK: usize = 4;
/// The block the pass appends first: the copy-on-write it inserts.
const INSERTED_ASK: usize = 8;

/// A nest whose inner loop hands the list to its copy-on-write entry point
/// on every iteration and writes an element:
/// `while outer: [OUTER_STEP] ; while inner: l = cow(l); l[j] = 0`. When
/// `outer_step` replaces the whole list, the outer loop writes it by
/// something other than the inner loop's questions, reading nothing of the
/// list it replaces.
fn nest_writing_a_list(outer_step: Vec<StatementKind>) -> Body {
    use crate::mir::optimization::stable_collections::{call, int, list_of, terminator, ty};

    let mut body = Body::new(0, span(), ExecutionModel::Cpu);
    body.local_decls = [
        ty(TypeKind::Int),
        ty(TypeKind::Int),
        list_of(TypeKind::Int),
        ty(TypeKind::Int),
        ty(TypeKind::Boolean),
        ty(TypeKind::Boolean),
        list_of(TypeKind::Int),
        list_of(TypeKind::Int),
    ]
    .into_iter()
    .map(|declared| LocalDecl::new(declared, span()))
    .collect();
    let goto = |target: usize| {
        terminator(TerminatorKind::Goto {
            target: BasicBlock(target),
        })
    };
    let switch = |below: Local, body_block: usize, exit: usize| {
        terminator(TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(below)),
            targets: vec![(Discriminant::bool_true(), BasicBlock(body_block))],
            otherwise: BasicBlock(exit),
        })
    };
    let block = |statements: Vec<StatementKind>, ending: Terminator| {
        let mut data = BasicBlockData::new(Some(ending));
        data.statements = statements
            .into_iter()
            .map(|kind| Statement { kind, span: span() })
            .collect();
        data
    };
    let element = Place {
        local: NEST_LIST,
        projection: vec![PlaceElem::Index(INNER_INDEX)],
    };
    body.basic_blocks = vec![
        block(Vec::new(), goto(OUTER_HEADER)),
        block(Vec::new(), switch(OUTER_BELOW, OUTER_STEP, 6)),
        block(outer_step, goto(INNER_HEADER)),
        block(Vec::new(), switch(INNER_BELOW, INNER_ASK, 5)),
        block(
            Vec::new(),
            call(
                Operand::runtime(rt::LIST_COW, span()),
                vec![Operand::Move(Place::new(NEST_LIST))],
                HANDED_BACK,
                7,
            ),
        ),
        block(Vec::new(), goto(OUTER_HEADER)),
        block(Vec::new(), terminator(TerminatorKind::Return)),
        block(
            vec![
                assign(
                    Place::new(NEST_LIST),
                    Rvalue::Use(Operand::Move(Place::new(HANDED_BACK))),
                ),
                assign(element, Rvalue::Use(int(0))),
                assign(
                    Place::new(INNER_INDEX),
                    Rvalue::Use(Operand::Copy(Place::new(OUTER_INDEX))),
                ),
            ],
            goto(INNER_HEADER),
        ),
    ];
    body
}

/// The header of the loop the copy-on-write `hoist_loop_unsharing` inserted
/// leads into, after checking it inserted exactly one.
fn hoisted_before(mut body: Body) -> usize {
    assert!(hoist_loop_unsharing(&mut body), "nothing hoisted:\n{body}");
    let asks = (body.basic_blocks.iter())
        .filter(|block| calls_runtime(block.terminator.as_ref(), rt::LIST_COW))
        .count();
    assert_eq!(asks, 2, "one copy-on-write inserted:\n{body}");
    let write_back = INSERTED_ASK + 1;
    match terminator(&body, write_back) {
        Some(TerminatorKind::Goto { target }) => target.0,
        other => panic!("bb{write_back} ends in {other:?}:\n{body}"),
    }
}

#[test]
fn test_a_nest_writing_a_list_only_by_its_questions_is_made_its_own_before_the_outer_loop() {
    assert_eq!(
        hoisted_before(nest_writing_a_list(Vec::new())),
        OUTER_HEADER
    );
}

#[test]
fn test_an_outer_loop_replacing_the_list_whole_keeps_the_copy_on_write_before_the_inner_loop() {
    let replace = assign(
        Place::new(NEST_LIST),
        Rvalue::Use(Operand::Move(Place::new(REPLACEMENT))),
    );
    assert_eq!(
        hoisted_before(nest_writing_a_list(vec![replace])),
        INNER_HEADER
    );
}

/// A runtime write to a list keeps the list known but not its elements: an
/// entry point writing a list in place may move or replace any element
/// (insert, set, remove), so the element the code asked about before the
/// write is asked about again after it.
#[test]
fn test_a_runtime_write_to_a_list_forgets_what_its_elements_were_known_to_be() {
    let with_write = with_cell(
        r#"
fn grow(a [int], b [int], c [int], i int) int
    var outer = List<[int]>()
    outer.push(a)
    outer.push(b)
    outer[i].push(1)
    outer.push(c)
    outer[i].push(2)
    outer[i].length()
"#,
    );
    let without_write = with_write.replace("    outer.push(c)\n", "");
    let checks = |source: &str| reachable_checks(&body_named(source, ".grow"));
    assert_eq!(checks(&with_write), checks(&without_write) + 1);
}
