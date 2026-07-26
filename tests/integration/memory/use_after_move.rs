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
    assert_runs_with_output(
        r#"
use system.memory
use system.collections.list

fn process(x [int])
    println(f"received={x.length()}")

let x = List([1, 2, 3])
process(x.clone())
println(f"original={x.length()}")
"#,
        "received=3\noriginal=3",
    );
}

// ─────────────────────────────────────────────
// Auto-copy struct: no clone needed
// ─────────────────────────────────────────────

#[test]
fn test_auto_copy_struct_exempt_from_move() {
    assert_runs_with_output(
        r#"

struct Point
    x float
    y float

fn process(p Point)
    println(f"received={p.x}")

let p = Point(1.0, 2.0)
process(p)
println(f"original={p.x}")
"#,
        "received=1.0\noriginal=1.0",
    );
}

// ─────────────────────────────────────────────
// String: use after move → error
// ─────────────────────────────────────────────

#[test]
fn test_string_use_after_move_error() {
    assert_compiler_error(
        r#"

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
    assert_runs_with_output(
        r#"
use system.collections.list

fn process(x [int])
    println(f"received={x.length()}")

let x = List([1, 2, 3])
process(x)
"#,
        "received=3",
    );
}

// ─────────────────────────────────────────────
// Primitive int: always auto-copy, no error
// ─────────────────────────────────────────────

#[test]
fn test_primitive_int_always_copied() {
    assert_runs_with_output(
        r#"

fn double(n int) int
    return n * 2

let n = 42
let d = double(n)
println(f"doubled={d}")
println(f"original={n}")
"#,
        "doubled=84\noriginal=42",
    );
}

// ─────────────────────────────────────────────
// Method receiver is not consumed
// ─────────────────────────────────────────────

#[test]
fn test_method_receiver_not_consumed() {
    assert_runs_with_output(
        r#"
use system.collections.list

let x = List([1, 2, 3])
let l = x.length()
println(f"first={l}")
let l2 = x.length()
println(f"second={l2}")
"#,
        "first=3\nsecond=3",
    );
}

// ─────────────────────────────────────────────
// Two separate vars: consuming one doesn't affect the other
// ─────────────────────────────────────────────

#[test]
fn test_consuming_one_var_does_not_affect_other() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn process(x [int])
    return

let x = List([1, 2, 3])
let y = List([4, 5, 6])
process(x)
println(f"y_len={y.length()}")
println(f"y_first={y.element_at(0)}")
"#,
        "y_len=3\ny_first=4",
    );
}

// ─────────────────────────────────────────────
// Custom class: use after move → error
// ─────────────────────────────────────────────

#[test]
fn test_custom_class_use_after_move_error() {
    assert_compiler_error(
        r#"

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
    assert_runs_with_output(
        r#"
use system.collections.list

fn process(x [int])
    println(f"processed={x.length()}")

var flag = true
let x = List([1, 2, 3])
if flag
    process(x)
println("after_if")
"#,
        "processed=3\nafter_if",
    );
}

// ─────────────────────────────────────────────
// If/else: consume only in then-branch → else branch must not see x as consumed
// ─────────────────────────────────────────────

#[test]
fn test_else_branch_not_poisoned_by_then_consume() {
    // `flag` is false so the else branch actually runs: the list it reads must
    // still be intact even though the then branch consumes it.
    assert_runs_with_output(
        r#"
use system.collections.list

fn process(x [int])
    return

var flag = false
let x = List([1, 2, 3])
if flag
    process(x)
else
    println(f"else_len={x.length()}")
"#,
        "else_len=3",
    );
}

// ─────────────────────────────────────────────
// If/else: consume in BOTH branches → use after if is an error
// ─────────────────────────────────────────────

#[test]
fn test_consume_in_both_branches_is_consumed_after_if() {
    assert_compiler_error(
        r#"
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

#[test]
fn test_resource_consumed_twice_in_function_body() {
    assert_compiler_error(
        r#"

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

#[test]
fn test_managed_type_not_consumed_in_function_body() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn step(l [int])
    println(f"step={l.length()}")

fn process(l [int])
    step(l)
    step(l)

process(List([1, 2, 3]))
println("done")
"#,
        "step=3\nstep=3\ndone",
    );
}

#[test]
fn test_resource_consumed_at_top_level_error() {
    assert_compiler_error(
        r#"

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

#[test]
fn test_resource_consumed_once_in_function_body_ok() {
    assert_runs_with_output(
        r#"

struct Res
    x int
    fn drop(self)
        return

fn sink(r Res)
    println(f"sunk={r.x}")

fn process(r Res)
    sink(r)

process(Res(x: 1))
println("done")
"#,
        "sunk=1\ndone",
    );
}

#[test]
fn test_resource_alias_then_use_error() {
    assert_compiler_error(
        r#"

struct Conn
    handle int
    fn drop(self)
        return

fn archive(c Conn)
    return

fn log_conn(c Conn)
    return

let c = Conn(handle: 1)
var a = c
archive(a)
log_conn(c)
"#,
        "consumed",
    );
}

#[test]
fn test_resource_alias_then_use_in_function_body_error() {
    assert_compiler_error(
        r#"

struct Conn
    handle int
    fn drop(self)
        return

fn sink(c Conn)
    return

fn handle_conn(c Conn)
    var alias = c
    sink(c)

handle_conn(Conn(handle: 1))
"#,
        "consumed",
    );
}

#[test]
fn test_managed_alias_compiles_cleanly() {
    assert_runs_with_output(
        r#"
use system.collections.list

let xs = List([1, 2, 3])
var ys = xs
println(f"original={xs.length()}")
println(f"alias={ys.length()}")
"#,
        "original=3\nalias=3",
    );
}

#[test]
fn test_resource_alias_reassignment_revives() {
    assert_runs_with_output(
        r#"

struct Conn
    handle int
    fn drop(self)
        return

fn sink(c Conn)
    println(f"sunk={c.handle}")

var a = Conn(handle: 1)
var b = a
sink(b)
a = Conn(handle: 2)
sink(a)
"#,
        "sunk=1\nsunk=2",
    );
}

#[test]
fn test_resource_passed_to_fn_consumed_error() {
    assert_compiler_error(
        r#"

struct Conn
    handle int
    fn drop(self)
        return

fn process(c Conn)
    return

let c = Conn(handle: 1)
process(c)
println(f"{c.handle}")
"#,
        "'c' was consumed by 'process'",
    );
}

#[test]
fn test_resource_consumed_in_body_second_use_error() {
    assert_compiler_error(
        r#"

struct Conn
    handle int
    fn drop(self)
        return

fn sink(c Conn)
    return

fn process(c Conn)
    sink(c)
    println(f"{c.handle}")

process(Conn(handle: 1))
"#,
        "'c' was consumed by 'sink'",
    );
}

#[test]
fn test_managed_type_in_function_body_no_error() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn helper(items [int])
    println(f"helper={items.element_at(0)}")

fn process(items [int])
    helper(items)
    helper(items)

process(List([7, 8, 9]))
println("done")
"#,
        "helper=7\nhelper=7\ndone",
    );
}

#[test]
fn test_resource_conditional_consume_no_else_compiles() {
    assert_runs_with_output(
        r#"

struct Conn
    handle int
    fn drop(self)
        return

fn sink(c Conn)
    println(f"sunk={c.handle}")

let cond = true
let c = Conn(handle: 1)
if cond
    sink(c)
println("after_if")
"#,
        "sunk=1\nafter_if",
    );
}

#[test]
fn test_dynamic_fn_param_callee_consumes_managed_arg() {
    assert_compiler_error(
        r#"
use system.collections.list

fn apply(items [int], f fn(xs [int]) int)
    f(items)
    println(f"{items.length()}")

apply(List([1, 2, 3]), fn(xs [int]) int: xs.length())
"#,
        "consumed",
    );
}

#[test]
fn test_dynamic_fn_param_diagnostic_names_dynamic_fn() {
    // The sink description must mention the dynamic-fn fallback so that the
    assert_compiler_error(
        r#"
use system.collections.list

fn apply(items [int], f fn(xs [int]) int)
    f(items)
    println(f"{items.length()}")

apply(List([1, 2, 3]), fn(xs [int]) int: xs.length())
"#,
        "dynamic fn",
    );
}

#[test]
fn test_dynamic_fn_let_bound_branch_consumes_managed_arg() {
    assert_compiler_error(
        r#"
use system.collections.list

fn save(xs [int]) int
    return xs.length()

fn noop(xs [int]) int
    return 0

fn process(items [int], cond bool)
    let target = if cond: save else: noop
    target(items)
    println(f"{items.length()}")

process(List([1, 2, 3]), true)
"#,
        "consumed",
    );
}

#[test]
fn test_dynamic_fn_param_clone_workaround_compiles() {
    assert_runs_with_output(
        r#"
use system.memory
use system.collections.list

fn apply(items [int], f fn(xs [int]) int)
    let n = f(items.clone())
    println(f"callee_saw={n}")
    println(f"still_alive={items.length()}")

apply(List([1, 2, 3]), fn(xs [int]) int: xs.length())
"#,
        "callee_saw=3\nstill_alive=3",
    );
}

#[test]
fn test_literal_free_fn_does_not_trigger_dynamic_fallback() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn helper(xs [int])
    println(f"helper={xs.length()}")

fn process(items [int])
    helper(items)
    println(f"still_alive={items.length()}")

process(List([1, 2, 3]))
"#,
        "helper=3\nstill_alive=3",
    );
}

#[test]
fn test_dynamic_fn_lambda_param_callee_consumes_managed_arg() {
    assert_compiler_error(
        r#"
use system.collections.list

fn run()
    let h = fn(g fn(xs [int]) int, items [int]) int
        let r = g(items)
        let unused = items.length()
        return r
    let _ = h(fn(xs [int]) int: xs.length(), List([1, 2, 3]))

run()
"#,
        "consumed",
    );
}

#[test]
fn test_dynamic_fn_for_loop_pattern_callee_consumes_managed_arg() {
    assert_compiler_error(
        r#"
use system.collections.list

fn run(fns [fn(xs [int]) int], items [int])
    for f in fns
        let _ = f(items)
    let _ = items.length()

run(List<fn(xs [int]) int>(), List([1, 2, 3]))
"#,
        "consumed",
    );
}

#[test]
fn test_dynamic_fn_for_loop_does_not_leak_binding_past_loop() {
    // After the for loop exits, `helper` should resolve back to the free fn —
    // the loop variable `helper` must not stay in `fn_bindings` and force
    // calls to the free `helper` to be classified as dynamic.
    assert_runs_with_output(
        r#"
use system.collections.list

fn helper(xs [int])
    println(f"helper={xs.length()}")

fn run(fns [fn(xs [int]) int], items [int])
    for helper in fns
        let _ = helper(items.clone())
    helper(items)
    println(f"still_alive={items.length()}")

run(List<fn(xs [int]) int>(), List([1, 2, 3]))
"#,
        "helper=3\nstill_alive=3",
    );
}
