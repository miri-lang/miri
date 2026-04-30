// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

// ─────────────────────────────────────────────
// List: use after move → error
// ─────────────────────────────────────────────

#[test]
fn test_list_use_after_move_error() {
    assert_compiler_error(
        r#"
use system.io
use system.collections.list

fn process(x [int])
    return

let x = List([1, 2, 3])
process(x)
println(f"{x.length()}")
"#,
        "consumed",
    );
}

// ─────────────────────────────────────────────
// List: clone fixes use after move
// ─────────────────────────────────────────────

#[test]
fn test_list_clone_fixes_use_after_move() {
    assert_runs(
        r#"
use system.io
use system.memory
use system.collections.list

fn process(x [int])
    return

let x = List([1, 2, 3])
process(x.clone())
println(f"{x.length()}")
"#,
    );
}

// ─────────────────────────────────────────────
// Auto-copy struct: no clone needed
// ─────────────────────────────────────────────

#[test]
fn test_auto_copy_struct_exempt_from_move() {
    assert_runs(
        r#"
use system.io

struct Point
    x float
    y float

fn process(p Point)
    return

let p = Point(1.0, 2.0)
process(p)
println(f"{p.x}")
"#,
    );
}

// ─────────────────────────────────────────────
// String: use after move → error
// ─────────────────────────────────────────────

#[test]
fn test_string_use_after_move_error() {
    assert_compiler_error(
        r#"
use system.io

fn consume(s String)
    return

let s = "hello"
consume(s)
println(s)
"#,
        "consumed",
    );
}

// ─────────────────────────────────────────────
// No move at all: compiles fine (single use)
// ─────────────────────────────────────────────

#[test]
fn test_no_move_compiles_fine() {
    assert_runs(
        r#"
use system.io
use system.collections.list

fn process(x [int])
    return

let x = List([1, 2, 3])
process(x)
"#,
    );
}

// ─────────────────────────────────────────────
// Primitive int: always auto-copy, no error
// ─────────────────────────────────────────────

#[test]
fn test_primitive_int_always_copied() {
    assert_runs(
        r#"
use system.io

fn double(n int) int
    return n * 2

let n = 42
double(n)
println(f"{n}")
"#,
    );
}

// ─────────────────────────────────────────────
// Method receiver is not consumed
// ─────────────────────────────────────────────

#[test]
fn test_method_receiver_not_consumed() {
    assert_runs(
        r#"
use system.io
use system.collections.list

let x = List([1, 2, 3])
let l = x.length()
println(f"{l}")
let l2 = x.length()
println(f"{l2}")
"#,
    );
}

// ─────────────────────────────────────────────
// Two separate vars: consuming one doesn't affect the other
// ─────────────────────────────────────────────

#[test]
fn test_consuming_one_var_does_not_affect_other() {
    assert_runs(
        r#"
use system.io
use system.collections.list

fn process(x [int])
    return

let x = List([1, 2, 3])
let y = List([4, 5, 6])
process(x)
println(f"{y.length()}")
"#,
    );
}

// ─────────────────────────────────────────────
// Custom class: use after move → error
// ─────────────────────────────────────────────

#[test]
fn test_custom_class_use_after_move_error() {
    assert_compiler_error(
        r#"
use system.io

class Buffer
    var data String

fn consume(b Buffer)
    return

let b = Buffer(data: "hello")
consume(b)
println(b.data)
"#,
        "consumed",
    );
}

// ─────────────────────────────────────────────
// Re-assignment revives a consumed variable
// ─────────────────────────────────────────────

#[test]
fn test_reassignment_revives_consumed_variable() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn process(x [int])
    return

var x = List([1, 2, 3])
process(x)
x = List([4, 5, 6])
println(f"{x.length()}")
"#,
        "3",
    );
}

// ─────────────────────────────────────────────
// Re-assign then consume again → use after second consume is still an error
// ─────────────────────────────────────────────

#[test]
fn test_use_after_move_error_still_reported_after_fresh_consume() {
    assert_compiler_error(
        r#"
use system.io
use system.collections.list

fn process(x [int])
    return

var x = List([1, 2, 3])
process(x)
x = List([4, 5, 6])
process(x)
println(f"{x.length()}")
"#,
        "consumed",
    );
}

// ─────────────────────────────────────────────
// Map<String, int>: use after move → error
// ─────────────────────────────────────────────

#[test]
fn test_map_use_after_move_error() {
    assert_compiler_error(
        r#"
use system.io
use system.collections.map

fn consume(m Map<String, int>)
    return

let m = {"a": 1, "b": 2}
consume(m)
println(f"{m.length()}")
"#,
        "consumed",
    );
}

// ─────────────────────────────────────────────
// Set<int>: use after move → error
// ─────────────────────────────────────────────

#[test]
fn test_set_use_after_move_error() {
    assert_compiler_error(
        r#"
use system.io
use system.collections.set

fn consume(s Set<int>)
    return

let s = {1, 2, 3}
consume(s)
println(f"{s.length()}")
"#,
        "consumed",
    );
}

// ─────────────────────────────────────────────
// If branch: consume only in then-branch → no error after if
// ─────────────────────────────────────────────

#[test]
fn test_consume_in_then_branch_only_not_flagged_after_if() {
    assert_runs(
        r#"
use system.io
use system.collections.list

fn process(x [int])
    return

var flag = true
let x = List([1, 2, 3])
if flag
    process(x)
println("ok")
"#,
    );
}

// ─────────────────────────────────────────────
// If/else: consume only in then-branch → else branch must not see x as consumed
// ─────────────────────────────────────────────

#[test]
fn test_else_branch_not_poisoned_by_then_consume() {
    assert_runs(
        r#"
use system.io
use system.collections.list

fn process(x [int])
    return

var flag = true
let x = List([1, 2, 3])
if flag
    process(x)
else
    println(f"{x.length()}")
"#,
    );
}

// ─────────────────────────────────────────────
// If/else: consume in BOTH branches → use after if is an error
// ─────────────────────────────────────────────

#[test]
fn test_consume_in_both_branches_is_consumed_after_if() {
    assert_compiler_error(
        r#"
use system.io
use system.collections.list

fn process(x [int])
    return

var flag = true
let x = List([1, 2, 3])
if flag
    process(x)
else
    process(x)
println(f"{x.length()}")
"#,
        "consumed",
    );
}

// ─────────────────────────────────────────────
// §7.4: Resource type consumed twice in function body → error
// ─────────────────────────────────────────────

#[test]
fn test_resource_consumed_twice_in_function_body() {
    assert_compiler_error(
        r#"
use system.io

struct Conn
    handle int
    fn drop(self)
        return

fn sink(c Conn)
    return

fn handle(c Conn)
    sink(c)
    sink(c)

handle(Conn(handle: 1))
"#,
        "consumed",
    );
}

// ─────────────────────────────────────────────
// §7.4: Managed-type function body — no error (no resource type)
// ─────────────────────────────────────────────

#[test]
fn test_managed_type_not_consumed_in_function_body() {
    assert_runs(
        r#"
use system.io
use system.collections.list

fn step(l [int])
    return

fn process(l [int])
    step(l)
    step(l)

process(List([1, 2, 3]))
println("ok")
"#,
    );
}

// ─────────────────────────────────────────────
// §7.4: Resource consumed at top level → error (unchanged from §7.1)
// ─────────────────────────────────────────────

#[test]
fn test_resource_consumed_at_top_level_error() {
    assert_compiler_error(
        r#"
use system.io

struct Res
    x int
    fn drop(self)
        return

fn sink(r Res)
    return

let r = Res(x: 1)
sink(r)
sink(r)
"#,
        "consumed",
    );
}

// ─────────────────────────────────────────────
// §7.4: Resource consumed once in function body → ok
// ─────────────────────────────────────────────

#[test]
fn test_resource_consumed_once_in_function_body_ok() {
    assert_runs(
        r#"
use system.io

struct Res
    x int
    fn drop(self)
        return

fn sink(r Res)
    return

fn process(r Res)
    sink(r)

process(Res(x: 1))
println("ok")
"#,
    );
}
