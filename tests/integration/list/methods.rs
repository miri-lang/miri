// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn list_push_pop() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List<int>()
l.push(10)
l.push(20)
println(f\"{l.length()}\")
println(f\"{l.pop()}\")
println(f\"{l.length()}\")
",
        "2\n20\n1",
    );
}

#[test]
fn list_insert_remove_at() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([1, 3])
l.insert(1, 2)
println(f\"{l[0]} {l[1]} {l[2]}\")
println(f\"{l.remove_at(1)}\")
println(f\"{l.length()}\")
",
        "1 2 3\n2\n2",
    );
}

#[test]
fn list_remove_by_value() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([10, 20, 30])
println(f\"{l.remove(20)}\")
println(f\"{l.remove(99)}\")
println(f\"{l.length()}\")
println(f\"{l[1]}\")
",
        "true\nfalse\n2\n30",
    );
}

#[test]
fn list_clear() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([1, 2, 3])
l.clear()
println(f\"{l.length()}\")
println(f\"{l.is_empty()}\")
",
        "0\ntrue",
    );
}

#[test]
fn list_reverse() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([1, 2, 3])
l.reverse()
println(f\"{l[0]} {l[1]} {l[2]}\")
",
        "3 2 1",
    );
}

#[test]
fn list_baselist_queries() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([10, 20, 30])
println(f\"{l.first() ?? -1}\")
println(f\"{l.last() ?? -1}\")
println(f\"{l.contains(20)}\")
println(f\"{l.index_of(30)}\")
println(f\"{l.last_index()}\")
",
        "10\n30\ntrue\n2\n2",
    );
}

#[test]
fn list_sort() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([30, 10, 20, 5])
l.sort()
println(f\"{l[0]} {l[1]} {l[2]} {l[3]}\")
",
        "5 10 20 30",
    );
}

#[test]
fn list_get_method() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([10, 20, 30])
println(f\"{l.get(0)}\")
println(f\"{l.get(2)}\")
",
        "10\n30",
    );
}

#[test]
fn list_set_method() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([10, 20, 30])
l.set(1, 99)
println(f\"{l[0]} {l[1]} {l[2]}\")
",
        "10 99 30",
    );
}

#[test]
fn list_element_at() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([10, 20, 30])
println(f\"{l.element_at(1)}\")
",
        "20",
    );
}

#[test]
fn list_remove_duplicate() {
    assert_runs_with_output(
        "
use system.io
use system.collections.list

let l = List([10, 20, 20, 30])
println(f\"{l.remove(20)}\")
println(f\"{l[0]} {l[1]} {l[2]}\")
println(f\"{l.length()}\")
",
        "true\n10 20 30\n3",
    );
}
