// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

// ─────────────────────────────────────────────
// List: use after move → error
// ─────────────────────────────────────────────

#[test]
fn test_list_passed_to_a_call_is_still_usable() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn process(x [int])
    return

let x = List([1, 2, 3])
process(x)
println(f"{x.length()}")
"#,
        "3",
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
fn test_string_passed_to_a_call_is_still_usable() {
    assert_heap_guard_output(
        r#"

fn consume(s String)
    return

let s = "hello"
consume(s)
println(s)
"#,
        "hello",
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
fn test_class_without_a_hook_passed_to_a_call_is_still_usable() {
    assert_heap_guard_output(
        r#"

class Buffer
    var data String

fn consume(b Buffer)
    return

let b = Buffer(data: "hello")
consume(b)
println(b.data)
"#,
        "hello",
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

class Res
    x int
    fn drop(self)
        return

fn keep(r Res) Res
    return r

var x = Res(x: 1)
let a = keep(x)
x = Res(x: 2)
let b = keep(x)
println(f"{x.x}")
"#,
        "consumed",
    );
}

// ─────────────────────────────────────────────
// Map<String, int>: use after move → error
// ─────────────────────────────────────────────

#[test]
fn test_map_passed_to_a_call_is_still_usable() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn consume(m Map<String, int>)
    return

let m = {"a": 1, "b": 2}
consume(m)
println(f"{m.length()}")
"#,
        "2",
    );
}

// ─────────────────────────────────────────────
// Set<int>: use after move → error
// ─────────────────────────────────────────────

#[test]
fn test_set_passed_to_a_call_is_still_usable() {
    assert_heap_guard_output(
        r#"
use system.collections.set

fn consume(s Set<int>)
    return

let s = {1, 2, 3}
consume(s)
println(f"{s.length()}")
"#,
        "3",
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

class Res
    x int
    fn drop(self)
        return

fn keep(r Res) Res
    return r

var flag = true
let x = Res(x: 1)
if flag
    let a = keep(x)
else
    let b = keep(x)
println(f"{x.x}")
"#,
        "consumed",
    );
}

#[test]
fn test_resource_consumed_twice_in_function_body() {
    assert_compiler_error(
        r#"

class Conn
    handle int
    fn drop(self)
        return

fn sink(c Conn) Conn
    return c

fn handle(c Conn)
    let a = sink(c)
    let b = sink(c)

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

class Res
    x int
    fn drop(self)
        return

fn sink(r Res) Res
    return r

let r = Res(x: 1)
let a = sink(r)
let b = sink(r)
"#,
        "consumed",
    );
}

#[test]
fn test_resource_consumed_once_in_function_body_ok() {
    assert_runs_with_output(
        r#"

class Res
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

class Conn
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

class Conn
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

class Conn
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

class Conn
    handle int
    fn drop(self)
        return

fn process(c Conn) Conn
    return c

let c = Conn(handle: 1)
let kept = process(c)
println(f"{c.handle}")
"#,
        "'c' was consumed by 'process'",
    );
}

#[test]
fn test_resource_consumed_in_body_second_use_error() {
    assert_compiler_error(
        r#"

class Conn
    handle int
    fn drop(self)
        return

fn sink(c Conn) Conn
    return c

fn process(c Conn)
    let kept = sink(c)
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

class Conn
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
fn test_dynamic_fn_param_callee_consumes_resource_arg() {
    assert_compiler_error(
        r#"

class Res
    x int
    fn drop(self)
        return

fn apply(r Res, f fn(r Res) int)
    f(r)
    println(f"{r.x}")

apply(Res(x: 1), fn(r Res) int: r.x)
"#,
        "consumed",
    );
}

#[test]
fn test_dynamic_fn_param_diagnostic_names_dynamic_fn() {
    // The sink description must mention the dynamic-fn fallback, so the
    // refusal says why a callee nobody can see into is assumed to keep it.
    assert_compiler_error(
        r#"

class Res
    x int
    fn drop(self)
        return

fn apply(r Res, f fn(r Res) int)
    f(r)
    println(f"{r.x}")

apply(Res(x: 1), fn(r Res) int: r.x)
"#,
        "dynamic fn",
    );
}

#[test]
fn test_dynamic_fn_let_bound_branch_consumes_resource_arg() {
    assert_compiler_error(
        r#"

class Res
    x int
    fn drop(self)
        return

fn save(r Res) int
    return r.x

fn noop(r Res) int
    return 0

fn process(r Res, cond bool)
    let target = if cond: save else: noop
    target(r)
    println(f"{r.x}")

process(Res(x: 1), true)
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
fn test_dynamic_fn_lambda_param_callee_consumes_resource_arg() {
    assert_compiler_error(
        r#"

class Res
    x int
    fn drop(self)
        return

fn run()
    let h = fn(g fn(r Res) int, r Res) int
        let n = g(r)
        let unused = r.x
        return n
    let _ = h(fn(r Res) int: r.x, Res(x: 1))

run()
"#,
        "consumed",
    );
}

#[test]
fn test_dynamic_fn_for_loop_pattern_callee_consumes_resource_arg() {
    assert_compiler_error(
        r#"
use system.collections.list

class Res
    x int
    fn drop(self)
        return

fn run(fns [fn(r Res) int], r Res)
    for f in fns
        let _ = f(r)
    let _ = r.x

run(List<fn(r Res) int>(), Res(x: 1))
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

#[test]
fn test_a_getter_returning_a_field_does_not_consume_the_receiver() {
    // Returning one of the receiver's fields hands out a reference to the field,
    // not the object, so the object is still readable afterwards. Treating it as
    // a consume made a method that composes two of its own reads unwritable.
    assert_runs_with_output(
        r#"
class Holder
    v String
    tag String

    fn init(v String, tag String)
        self.v = v
        self.tag = tag

    public fn get() String
        return self.v

    public fn both() String
        return self.get() + self.tag

fn main()
    println(Holder("A".to_lower(), "B".to_lower()).both())
"#,
        "ab",
    );
}

#[test]
fn test_a_getter_called_at_a_call_site_does_not_consume_the_binding() {
    assert_runs_with_output(
        r#"
class Holder
    v String
    tag String

    fn init(v String, tag String)
        self.v = v
        self.tag = tag

    public fn get() String
        return self.v

fn main()
    let o = Holder("A".to_lower(), "B".to_lower())
    println(o.get() + o.tag)
    println(o.get())
"#,
        "ab
a",
    );
}

#[test]
fn test_a_method_returning_the_receiver_leaves_a_value_receiver_usable() {
    // A class instance without a hook is a value: the method returns a copy
    // of its receiver, and the caller's binding still holds its own.
    assert_heap_guard_output(
        r#"
use system.collections.list

class Holder
    items [int]

    fn init(items [int])
        self.items = items

    public fn into_self() Holder
        return self

fn main()
    let o = Holder(List([1, 2]))
    let taken = o.into_self()
    println(f"{o.items.length()} {taken.items.length()}")
"#,
        "2 2",
    );
}

#[test]
fn test_a_getter_result_outliving_its_receiver_is_still_valid() {
    // The field reference the getter hands back is independent of the object it
    // came from, which is what makes not consuming the receiver sound.
    assert_runs_with_output(
        r#"
class Holder
    v String

    fn init(v String)
        self.v = v

    public fn get() String
        return self.v

fn main()
    var kept = ""
    if true
        let h = Holder("a" + "b")
        kept = h.get()
    println(kept)
"#,
        "ab",
    );
}

#[test]
fn test_calling_a_getter_twice_hands_back_the_field_each_time() {
    // Each call retains the field it returns, so two results coexist and the
    // object is still whole afterwards. This is the shape that would double-free
    // if the reference the getter hands out were not its own.
    assert_runs_with_output(
        r#"
use system.collections.list

class Holder
    items [int]

    fn init(items [int])
        self.items = items

    public fn take() [int]
        return self.items

fn main()
    let o = Holder(List([1, 2]))
    let a = o.take()
    let b = o.take()
    println(f"{a.length()} {b.length()} {o.items.length()}")
"#,
        "2 2 2",
    );
}

#[test]
fn test_a_whole_value_handed_to_a_consuming_function_is_still_refused() {
    // The projection rule must not soften the ordinary case: a resource passed
    // on as itself to a callee that keeps it is gone, and using it afterwards
    // is still an error.
    assert_compiler_error(
        r#"

class Res
    x int
    fn drop(self)
        return

fn sink(r Res) Res
    return r

fn main()
    let l = Res(x: 1)
    let taken = sink(l)
    println(f"{l.x}")
"#,
        "was consumed by",
    );
}

const HANDLE: &str = r#"
class Handle
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

fn close(h Handle) Handle
    println("closing")
    return h
"#;

/// A resource consumed on every pass of a loop is used again on the second
/// pass, after the first consumed it.
#[test]
fn a_resource_consumed_inside_a_loop_is_refused_on_the_next_pass() {
    assert_compiler_error(
        &format!(
            "{HANDLE}\n{}",
            r#"
fn main()
    let h = Handle(1)
    var i = 0
    while i < 2
        let _closed = close(h)
        i = i + 1
"#
        ),
        "MER_OWN_003",
    );
}

/// Consuming it on the way out of the loop leaves no next pass to use it.
// TODO: assert the hook order exactly, "closing\nclosed 1\ndone". The handle
// `close` returns is owned by `_closed`, whose scope ends at the `break`, but a
// call does not yet move a consumed resource's count to the callee, so the hook
// still runs when `h`'s scope ends.
#[test]
fn a_resource_consumed_before_leaving_a_loop_is_accepted() {
    assert_runs_with_output(
        &format!(
            "{HANDLE}\n{}",
            r#"
fn main()
    let h = Handle(1)
    var i = 0
    while i < 2
        if i == 0
            let _closed = close(h)
            break
        i = i + 1
    println("done")
"#
        ),
        "closing\ndone",
    );
}

/// An enum's drop hook runs once when its value is released, even when the
/// hook reads `self` as a whole value.
#[test]
fn an_enums_drop_hook_runs_once_at_release() {
    assert_heap_guard_output(
        r#"
enum Res
    Tag(int)

    fn drop(self)
        match self
            Res.Tag(n): println(f"drop {n}")

fn main()
    if true
        let r = Res.Tag(1)
        println("inside")
    println("end")
"#,
        "inside\ndrop 1\nend",
    );
}

/// A call lends a list at the top level as it does inside a function: the
/// list is still the caller's after a callee that keeps a copy of it.
#[test]
fn a_list_a_callee_keeps_is_still_usable_at_the_top_level() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn keep(xs [int]) [int]
    return xs

let a = List([1, 2, 3])
let b = keep(a)
println(f"{a.length()} {b.length()}")
"#,
        "3 3",
    );
}

/// The same statements inside `main` behave exactly as at the top level.
#[test]
fn a_list_a_callee_keeps_is_still_usable_inside_a_function() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn keep(xs [int]) [int]
    return xs

fn main()
    let a = List([1, 2, 3])
    let b = keep(a)
    println(f"{a.length()} {b.length()}")
"#,
        "3 3",
    );
}

/// A list passed twice in one call, at the top level, is lent twice.
#[test]
fn a_list_passed_twice_to_one_call_is_lent_at_the_top_level() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn bump(x [int], y [int]) int
    return x.length() + y.length()

var a = List([1, 2, 3])
println(f"{bump(a, a)}")
println(f"{a.length()}")
"#,
        "6\n3",
    );
}

/// A write the callee makes to the copy it kept is never seen by the caller.
#[test]
fn a_kept_list_written_by_the_callee_leaves_the_callers_list_unchanged() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn grow(xs [int]) [int]
    var ys = xs
    ys.push(4)
    return ys

fn main()
    let a = List([1, 2, 3])
    let b = grow(a)
    println(f"{a.length()} {b.length()}")
"#,
        "3 4",
    );
}

/// A string a method stores is still the caller's after the call.
#[test]
fn a_string_a_method_stores_is_still_usable() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    var names = List<String>()
    let s = "a" + "b"
    names.push(s)
    println(s)
    println(names[0])
"#,
        "ab\nab",
    );
}

/// A list handed to a function value is lent, not consumed.
#[test]
fn a_list_passed_to_a_function_value_is_still_usable() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let f = fn(xs [int]) [int]: xs
    let a = List([1, 2])
    let b = f(a)
    println(f"{a.length()} {b.length()}")
"#,
        "2 2",
    );
}

/// A resource lent to a function that only reads it stays the caller's, and
/// its hook runs once, at the caller's scope end, in every scope.
#[test]
fn a_resource_a_callee_only_reads_is_lent_at_the_top_level() {
    assert_heap_guard_output(
        r#"
class Res
    x int
    fn drop(self)
        println("dropped")

fn look(r Res)
    println(f"look {r.x}")

let r = Res(x: 1)
look(r)
look(r)
println("end")
"#,
        "look 1\nlook 1\nend\ndropped",
    );
}

/// Inside a function, a resource lent twice runs its hook once, at scope end.
#[test]
fn a_resource_a_callee_only_reads_is_lent_inside_a_function() {
    assert_heap_guard_output(
        r#"
class Res
    x int
    fn drop(self)
        println("dropped")

fn look(r Res)
    println(f"look {r.x}")

fn main()
    let r = Res(x: 1)
    look(r)
    look(r)
    println("end")
"#,
        "look 1\nlook 1\nend\ndropped",
    );
}

/// A resource a callee keeps is consumed by the call, at the top level, and the
/// refusal names the callee without suggesting a copy.
#[test]
fn a_resource_a_callee_keeps_is_consumed_at_the_top_level() {
    let output = crate::utils::miri_check(
        r#"
class Res
    x int
    fn drop(self)
        println("dropped")

fn keep(r Res) Res
    return r

let r = Res(x: 1)
let k = keep(r)
println(f"{r.x}")
"#,
    )
    .output();
    assert!(
        output.contains("'r' was consumed by 'keep'"),
        "expected the consuming callee named:\n{output}"
    );
    assert!(
        !output.contains(".clone()"),
        "a resource has one owner, so a copy is not the repair:\n{output}"
    );
}

/// The same refusal inside a function.
#[test]
fn a_resource_a_callee_keeps_is_consumed_inside_a_function() {
    assert_compiler_error(
        r#"
class Res
    x int
    fn drop(self)
        println("dropped")

fn keep(r Res) Res
    return r

fn main()
    let r = Res(x: 1)
    let k = keep(r)
    println(f"{r.x}")
"#,
        "'r' was consumed by 'keep'",
    );
}
