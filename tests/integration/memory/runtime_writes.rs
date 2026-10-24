// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A runtime entry point a program declares and calls itself writes the
//! collection it is handed in place, so the call is a write like any other:
//! a collection another binding shares is copied first and the write lands in
//! the copy the named binding holds, and a parameter written that way is a
//! copy the function binds on entry, leaving its caller's value alone.

use super::super::utils::*;

#[test]
fn test_a_runtime_writer_on_a_shared_list_leaves_the_other_binding_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

runtime "core" fn miri_rt_list_sort_by(l [int], f fn(int, int) int)

fn main()
    var l = List([3, 1, 2])
    let keep = l
    miri_rt_list_sort_by(l, fn(a int, b int) int: b - a)
    println(f"{l[0]} {l[1]} {l[2]} / {keep[0]} {keep[1]} {keep[2]}")
"#,
        "3 2 1 / 3 1 2",
    );
}

#[test]
fn test_a_runtime_writer_on_a_parameter_leaves_the_callers_list_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

runtime "core" fn miri_rt_list_sort_by(l [int], f fn(int, int) int)

fn sort_desc(l [int]) int
    miri_rt_list_sort_by(l, fn(a int, b int) int: b - a)
    return l[0]

fn main()
    var l = List([1, 3, 2])
    let first = sort_desc(l)
    println(f"{first} / {l[0]} {l[1]} {l[2]}")
"#,
        "3 / 1 3 2",
    );
}

#[test]
fn test_a_runtime_writer_on_an_unshared_list_writes_it_in_place() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

runtime "core" fn miri_rt_list_clear(l [int])

fn main()
    var l = List([1, 2, 3])
    miri_rt_list_clear(l)
    l.push(9)
    println(f"{l.length()} {l[0]}")
"#,
        "1 9",
    );
}

/// The program declaring `miri_rt_list_clear`, binding `setup` in `main`,
/// clearing `cleared` and printing `report`.
fn clearing(setup: &str, cleared: &str, report: &str) -> String {
    format!(
        r#"
use system.io
use system.collections.list
use system.collections.map

runtime "core" fn miri_rt_list_clear(l [int])

struct S
    items [int]

fn make() [int]
    return List([7, 8])

fn main()
    var a = List([1, 2, 3])
{setup}
    miri_rt_list_clear({cleared})
    println(f"{report}")
"#
    )
}

#[test]
fn test_a_runtime_writer_on_a_tuple_element_leaves_the_list_it_was_built_from() {
    assert_runs_with_output(
        &clearing("    var t = (a, 1)", "t.0", "{a.length()} {t.0.length()}"),
        "3 0",
    );
}

#[test]
fn test_a_runtime_writer_on_a_struct_field_leaves_the_list_it_was_built_from() {
    assert_runs_with_output(
        &clearing(
            "    var s = S(items: a)\n    let keep = s",
            "s.items",
            "{a.length()} {s.items.length()} {keep.items.length()}",
        ),
        "3 0 3",
    );
}

#[test]
fn test_a_runtime_writer_on_a_list_element_leaves_its_sibling_alone() {
    assert_runs_with_output(
        &clearing(
            "    var n = List([a, a])\n    let keep = n",
            "n[0]",
            "{a.length()} {n[0].length()} {n[1].length()} {keep[0].length()}",
        ),
        "3 0 3 3",
    );
}

#[test]
fn test_a_runtime_writer_on_a_map_entry_leaves_the_other_map_alone() {
    assert_runs_with_output(
        &clearing(
            "    var m = Map<int, [int]>()\n    m[1] = a\n    let keep = m",
            "m[1]",
            "{a.length()} {m[1].length()} {keep[1].length()}",
        ),
        "3 0 3",
    );
}

#[test]
fn test_a_runtime_writer_on_a_call_result_releases_it_once() {
    assert_runs_with_output(&clearing("", "make()", "{a.length()}"), "3");
}

/// A runtime entry point is reached only by its own name in the module that
/// declares it: no module can call one through an alias, so the scan for the
/// parameters a body writes need only read calls by a plain name.
#[test]
fn test_a_runtime_writer_cannot_be_called_through_a_module_alias() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use system.collections.list\n",
                    "use local.lists as L\n",
                    "\n",
                    "fn main()\n",
                    "    var a = List([1, 2, 3])\n",
                    "    L.miri_rt_list_clear(a)\n",
                ),
            ),
            (
                "lists.mi",
                "runtime \"core\" fn miri_rt_list_clear(l [int])\n",
            ),
        ],
        "'miri_rt_list_clear' is not visible",
    );
}

/// The list a runtime writer is handed is made the call's own before the
/// arguments after it are lowered, so a closure argument capturing the same
/// list holds it again while the runtime writes it. The sort detaches the
/// list's elements for the duration of the call, so the closure finds it
/// empty and its index stops the program cleanly.
#[test]
fn test_a_closure_argument_capturing_the_written_list_finds_it_empty_during_the_write() {
    assert_runtime_error(
        r#"
use system.io
use system.collections.list

runtime "core" fn miri_rt_list_sort_by(l [int], f fn(int, int) int)

fn main()
    var l = List([3, 1, 2])
    miri_rt_list_sort_by(l, fn(a int, b int) int: a - b + l[0] - l[0])
    println(f"{l[0]} {l[1]} {l[2]}")
"#,
        "Array index out of bounds: the len is 0 but the index is 0",
    );
}
