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

/// Consuming it on the way out of the loop leaves no next pass to use it. The
/// handle `close` returns is owned by `_closed`, whose scope ends at the
/// `break`, so the hook runs before the loop is left.
#[test]
fn a_resource_consumed_before_leaving_a_loop_is_accepted() {
    assert_heap_guard_output(
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
        "closing\nclosed 1\ndone",
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

/// A resource moved to another binding in one arm of an `if` may be gone
/// after it, so a later use is refused.
#[test]
fn a_resource_moved_in_one_arm_is_refused_after_the_if() {
    assert_compiler_error(
        &format!(
            "{HANDLE}\n{}",
            r#"
fn main()
    let h = Handle(1)
    if true
        let _k = h
    println(f"{h.id}")
"#
        ),
        "MER_OWN_003",
    );
}

/// The same for a call that keeps it in one arm.
#[test]
fn a_resource_a_call_keeps_in_one_arm_is_refused_after_the_if() {
    assert_compiler_error(
        &format!(
            "{HANDLE}\n{}",
            r#"
fn main()
    let h = Handle(1)
    if true
        let _closed = close(h)
    println(f"{h.id}")
"#
        ),
        "'h' was consumed by 'close' and cannot be used again",
    );
}

/// And for a method that keeps its receiver.
#[test]
fn a_receiver_a_method_keeps_in_one_arm_is_refused_after_the_if() {
    assert_compiler_error(
        r#"
class Token
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
    fn keep() Token
        return self

fn main()
    let h = Token(1)
    if true
        let _k = h.keep()
    println(f"{h.id}")
"#,
        "MER_OWN_003",
    );
}

/// One `match` arm consuming it is enough to refuse a use after the match.
#[test]
fn a_resource_consumed_in_one_match_arm_is_refused_after_the_match() {
    assert_compiler_error(
        &format!(
            "{HANDLE}\n{}",
            r#"
fn run(n int)
    let h = Handle(1)
    match n
        1: println(f"{close(h).id}")
        _: println("other")
    println(f"{h.id}")

fn main()
    run(1)
"#
        ),
        "MER_OWN_003",
    );
}

/// An arm that consumes it and returns never reaches the use after the `if`.
#[test]
fn a_resource_consumed_in_an_arm_that_returns_is_usable_after_the_if() {
    assert_heap_guard_output(
        &format!(
            "{HANDLE}\n{}",
            r#"
fn run(flag bool, id int)
    let h = Handle(id)
    if flag
        let _closed = close(h)
        return
    println(f"still {h.id}")

fn main()
    run(true, 1)
    run(false, 2)
"#
        ),
        "closing\nclosed 1\nstill 2\nclosed 2",
    );
}

/// A conditional expression consuming it in one branch may consume it.
#[test]
fn a_resource_consumed_in_one_branch_of_a_conditional_expression_is_refused_after_it() {
    assert_compiler_error(
        &format!(
            "{HANDLE}\n{}",
            r#"
fn run(flag bool)
    let h = Handle(1)
    let k = close(h) if flag else Handle(2)
    println(f"{h.id} {k.id}")

fn main()
    run(true)
"#
        ),
        "MER_OWN_003",
    );
}

/// Each branch of a conditional expression consuming it once is one consume
/// per path, not two.
#[test]
fn a_resource_consumed_in_both_branches_of_a_conditional_expression_is_accepted() {
    assert_heap_guard_output(
        &format!(
            "{HANDLE}\n{}",
            r#"
fn run(flag bool)
    let h = Handle(1)
    let k = close(h) if flag else close(h)
    println(f"got {k.id}")

fn main()
    run(true)
"#
        ),
        "closing\ngot 1\nclosed 1",
    );
}

/// A loop that can leave through a `break` elsewhere still runs again after
/// a pass that consumed it without leaving.
#[test]
fn a_resource_consumed_on_a_pass_that_continues_is_refused_in_a_loop_that_can_break() {
    assert_compiler_error(
        &format!(
            "{HANDLE}\n{}",
            r#"
fn main()
    let h = Handle(1)
    var i = 0
    while i < 2
        if i == 5
            break
        let _closed = close(h)
        i = i + 1
"#
        ),
        "MER_OWN_003",
    );
}

/// A `continue` carries what its pass consumed into the next pass.
#[test]
fn a_resource_consumed_before_a_continue_is_refused_on_the_next_pass() {
    assert_compiler_error(
        &format!(
            "{HANDLE}\n{}",
            r#"
fn main()
    let h = Handle(1)
    var i = 0
    while i < 2
        i = i + 1
        if i == 1
            let _closed = close(h)
            continue
        println(f"{h.id}")
"#
        ),
        "MER_OWN_003",
    );
}

/// Consumed on the way out of the loop, it is gone after the loop.
#[test]
fn a_resource_consumed_before_a_break_is_refused_after_the_loop() {
    assert_compiler_error(
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
    println(f"{h.id}")
"#
        ),
        "MER_OWN_003",
    );
}

/// A block's own binding of a name ends with the block: the outer binding it
/// shadowed is consumed after the block exactly when it was before it.
#[test]
fn a_shadowing_binding_leaves_the_outer_bindings_consumption_alone() {
    assert_compiler_error(
        &format!(
            "{HANDLE}\n{}",
            r#"
fn main()
    let h = Handle(1)
    let _k = h
    if true
        let h = Handle(2)
        println(f"{h.id}")
    println(f"{h.id}")
"#
        ),
        "MER_OWN_003",
    );
}

/// The `HANDLE` program followed by `program`.
fn with_handle(program: &str) -> String {
    format!("{HANDLE}\n{program}")
}

/// A `while` condition runs again before every pass, so a resource it
/// consumes is gone when it runs the second time.
#[test]
fn a_resource_consumed_by_a_while_condition_is_refused_on_the_next_pass() {
    assert_compiler_error(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    var i = 0
    while close(h).id > i
        i = i + 1
    println("done")
"#,
        ),
        "MER_OWN_003",
    );
}

/// The right operand of `and` consumes on every pass that reaches it.
#[test]
fn a_resource_consumed_in_a_while_conditions_right_operand_is_refused() {
    assert_compiler_error(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    var i = 0
    while i < 3 and close(h).id > 0
        i = i + 1
    println("done")
"#,
        ),
        "MER_OWN_003",
    );
}

/// A condition that only reads the resource runs on every pass.
#[test]
fn a_while_condition_that_only_reads_a_resource_is_accepted() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let h = Handle(2)
    var i = 0
    while h.id > i
        i = i + 1
    println(f"done {i}")
"#,
        ),
        "done 2\nclosed 2",
    );
}

/// A `for` evaluates its iterable once, so reading a resource there is
/// accepted however many passes the loop makes.
#[test]
fn a_for_iterable_is_evaluated_once() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let h = Handle(3)
    for i in 0..h.id
        println(f"{i}")
    println("done")
"#,
        ),
        "0\n1\n2\ndone\nclosed 3",
    );
}

/// A match arm's binding of a name is the arm's own: consuming it leaves the
/// outer binding of the same name alone.
// TODO: assert "closing\nclosed 2\n1\nclosed 1". The type checker leaves the
// outer `h` owned, but lowering does not release a payload binding the arm's
// call consumed where `_c` goes out of scope: the option `o` still holds the
// payload, so the hook runs when `o` is released at the end of `main`.
#[test]
fn a_match_arm_binding_that_shadows_a_resource_leaves_the_outer_one_alone() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    let o Handle? = Some(Handle(2))
    match o
        Some(h)
            let _c = close(h)
        None: println("none")
    println(f"{h.id}")
"#,
        ),
        "closing\n1\nclosed 2\nclosed 1",
    );
}

/// `if let` binds its name the same way a match arm does.
// TODO: assert "closing\nclosed 2\n1\nclosed 1". The type checker leaves the
// outer `h` owned, but lowering does not release a payload binding the arm's
// call consumed where `_c` goes out of scope: the option `o` still holds the
// payload, so the hook runs when `o` is released at the end of `main`.
#[test]
fn an_if_let_binding_that_shadows_a_resource_leaves_the_outer_one_alone() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    let o Handle? = Some(Handle(2))
    if let Some(h) = o
        let _c = close(h)
    println(f"{h.id}")
"#,
        ),
        "closing\n1\nclosed 2\nclosed 1",
    );
}

/// The outer resource consumed inside an arm is consumed after the match.
#[test]
fn an_outer_resource_consumed_in_a_match_arm_is_refused_after_the_match() {
    assert_compiler_error(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    let o Handle? = Some(Handle(2))
    match o
        Some(x)
            let _c = close(h)
        None: println("none")
    println(f"{h.id}")
"#,
        ),
        "MER_OWN_003",
    );
}

/// Nested loops that each consume a resource of the loop around them on
/// every pass. Checking costs one walk per loop for each loop around it,
/// not one for every combination of passes, so twenty-four levels check
/// well inside the budget.
#[test]
fn deeply_nested_loops_that_consume_check_in_bounded_time() {
    use std::io::Write;
    const DEPTH: usize = 24;
    const BUDGET: std::time::Duration = std::time::Duration::from_secs(60);
    let mut program = String::from(HANDLE);
    program.push_str("\nfn main()\n    let r0 = Handle(0)\n");
    for level in 1..=DEPTH {
        let indent = "    ".repeat(level);
        program.push_str(&format!(
            "{indent}while true\n{indent}    let r{level} = Handle({level})\n{indent}    let _c{level} = close(r{previous})\n",
            previous = level - 1
        ));
    }
    let mut file = tempfile::NamedTempFile::new().unwrap();
    write!(file, "{program}").unwrap();
    let stdlib_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");
    let started = std::time::Instant::now();
    let output = crate::utils::miri_cmd()
        .env("MIRI_STDLIB_PATH", stdlib_path)
        .arg("check")
        .arg(file.path())
        .timeout(BUDGET)
        .output()
        .unwrap();
    let report = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        report.contains("MER_OWN_003"),
        "expected MER_OWN_003 within {BUDGET:?}, got {:?} after {:?}:\n{report}",
        output.status,
        started.elapsed(),
    );
}

/// Twelve nested loops that consume nothing check clean.
#[test]
fn twelve_nested_loops_that_consume_nothing_check_clean() {
    let mut program = String::from(HANDLE);
    program.push_str("\nfn main()\n    let h = Handle(1)\n    var n = 0\n");
    for level in 1..=12 {
        let indent = "    ".repeat(level);
        program.push_str(&format!("{indent}for i{level} in 0..1\n"));
    }
    program.push_str(&format!("{}n = n + h.id\n", "    ".repeat(13)));
    program.push_str("    println(f\"{n}\")\n");
    assert_heap_guard_output(&program, "1\nclosed 1");
}

/// Programs the rule accepts: on every path that reaches the use, the
/// resource is still owned. Each must check clean and run with this output.
const ACCEPTED: &[(&str, &str, &str)] = &[
    (
        "consumed in an arm that returns, then used",
        r#"
fn run(flag bool)
    let h = Handle(1)
    if flag
        let _c = close(h)
        return
    println(f"use {h.id}")

fn main()
    run(false)
    run(true)
"#,
        "use 1\nclosed 1\nclosing\nclosed 1",
    ),
    (
        "re-assigned after a conditional join",
        r#"
fn run(flag bool)
    var h = Handle(1)
    if flag
        let _c = close(h)
    h = Handle(2)
    println(f"use {h.id}")

fn main()
    run(true)
"#,
        "closing\nclosed 1\nuse 2\nclosed 2",
    ),
    (
        "re-assigned inside a loop body",
        r#"
fn main()
    var h = Handle(1)
    var i = 0
    while i < 2
        let _c = close(h)
        h = Handle(i + 10)
        i = i + 1
    println(f"last {h.id}")
"#,
        "closing\nclosed 1\nclosing\nclosed 10\nlast 11\nclosed 11",
    ),
    (
        "consumed in an arm that panics, then used",
        r#"
fn run(flag bool)
    let h = Handle(1)
    if flag
        let _c = close(h)
        panic("boom")
    println(f"use {h.id}")

fn main()
    run(false)
"#,
        "use 1\nclosed 1",
    ),
    (
        "consumed in a match arm that panics, then used",
        r#"
fn run(o int?)
    let h = Handle(1)
    match o
        Some(n)
            let _c = close(h)
            panic(f"{n}")
        None: println("none")
    println(f"use {h.id}")

fn main()
    run(None)
"#,
        "none\nuse 1\nclosed 1",
    ),
    (
        "a for binding and a lambda parameter shadow a consumed resource",
        r#"
fn main()
    let h = Handle(1)
    let k = h
    for h in 0..2
        println(f"{h}")
    let f = fn(h int) int: h + 1
    println(f"{f(4)} {k.id}")
"#,
        "0\n1\n5 1\nclosed 1",
    ),
    (
        "a match guard reads the resource",
        r#"
fn main()
    let h = Handle(1)
    let o int? = Some(3)
    match o
        Some(n) if n > h.id: println(f"big {n}")
        Some(n): println(f"small {n}")
        None: println("none")
    println(f"use {h.id}")
"#,
        "big 3\nuse 1\nclosed 1",
    ),
    (
        "declared and consumed inside a loop body",
        r#"
fn main()
    var i = 0
    while i < 2
        let h = Handle(i)
        let _c = close(h)
        i = i + 1
    println("done")
"#,
        "closing\nclosed 0\nclosing\nclosed 1\ndone",
    ),
    (
        "an else-if chain whose consuming arms return",
        r#"
fn run(n int)
    let h = Handle(n)
    if n == 1
        let _c = close(h)
        return
    else if n == 2
        let _c = close(h)
        return
    else
        println("kept")
    println(f"use {h.id}")

fn main()
    run(1)
    run(2)
    run(3)
"#,
        "closing\nclosed 1\nclosing\nclosed 2\nkept\nuse 3\nclosed 3",
    ),
    (
        "an inner break leaves only the inner loop",
        r#"
fn main()
    var i = 0
    while i < 2
        i = i + 1
        let h = Handle(i)
        while true
            let _c = close(h)
            break
        println("next")
"#,
        "closing\nclosed 1\nnext\nclosing\nclosed 2\nnext",
    ),
];

#[test]
fn programs_whose_uses_are_reached_only_while_the_resource_is_owned_are_accepted() {
    for (case, program, expected) in ACCEPTED {
        eprintln!("case: {case}");
        assert_heap_guard_output(&with_handle(program), expected);
    }
}

/// Programs the rule refuses: some path consumes the resource before the use.
const REFUSED: &[(&str, &str)] = &[
    (
        "consumed by the right operand of ??, then used",
        r#"
fn main()
    let h = Handle(1)
    let o Handle? = None
    let k = o ?? close(h)
    println(f"{k.id} {h.id}")
"#,
    ),
    (
        "consumed by the else operand of a conditional expression, then used",
        r#"
fn run(flag bool)
    let h = Handle(1)
    let k = Handle(2) if flag else close(h)
    println(f"{k.id} {h.id}")
"#,
    ),
    (
        "consumed before an inner continue, then used on the inner loop's next pass",
        r#"
fn main()
    let h = Handle(1)
    var j = 0
    while j < 2
        j = j + 1
        if j == 1
            let _c = close(h)
            continue
        println(f"{h.id}")
"#,
    ),
    (
        "consumed before an inner continue, then used after the inner loop",
        r#"
fn main()
    let h = Handle(1)
    var j = 0
    while j < 2
        j = j + 1
        if j == 1
            let _c = close(h)
            continue
    println(f"{h.id}")
"#,
    ),
    (
        "consumed before an inner break, then consumed again on the outer loop's next pass",
        r#"
fn main()
    let h = Handle(1)
    var i = 0
    while i < 2
        i = i + 1
        while true
            let _c = close(h)
            break
"#,
    ),
];

#[test]
fn programs_that_use_a_resource_some_path_consumed_are_refused() {
    for (case, program) in REFUSED {
        eprintln!("case: {case}");
        assert_compiler_error(&with_handle(program), "MER_OWN_003");
    }
}

/// A conditional expression's branch that never ends reaches no join, the
/// same as an `if` or `match` arm that never finishes.
#[test]
fn a_resource_consumed_in_a_conditional_branch_that_never_ends_is_still_owned_after_it() {
    assert_heap_guard_output(
        &with_handle(
            r#"
@diverges
fn give_up(h Handle) Handle
    panic(f"gave up {h.id}")

fn run(flag bool)
    let h = Handle(1)
    let k = Handle(2) if flag else give_up(close(h))
    println(f"{k.id} {h.id}")

fn main()
    run(true)
"#,
        ),
        "2 1\nclosed 2\nclosed 1",
    );
}

const ONE_RESOURCE_TWO_POSITIONS: &str = r#"
class Res
    var x int
    fn drop(self)
        println(f"drop {self.x}")

fn keep_both(a Res, b Res) Res
    if a.x > b.x
        return a
    return b

fn keep_first(a Res, b Res) Res
    println(f"reads {b.x}")
    return a

fn read_both(a Res, b Res) int
    return a.x + b.x

class Picker
    var bias int
    fn pick(a Res, b Res) Res
        if self.bias > 0
            return a
        return b
"#;

/// A resource has one owner, so a call cannot keep it at two positions.
#[test]
fn a_resource_kept_at_two_positions_of_one_call_is_refused() {
    assert_compiler_error(
        &format!(
            "{ONE_RESOURCE_TWO_POSITIONS}
fn main()
    let h = Res(x: 1)
    let p = keep_both(h, h)
"
        ),
        "'h' is passed to 'keep_both' more than once",
    );
}

/// A call cannot lend a resource it also keeps: the lent view would outlive
/// the owner the callee becomes.
#[test]
fn a_resource_kept_at_one_position_and_lent_at_another_is_refused() {
    assert_compiler_error(
        &format!(
            "{ONE_RESOURCE_TWO_POSITIONS}
fn main()
    let h = Res(x: 1)
    let k = keep_first(h, h)
"
        ),
        "'h' is passed to 'keep_first' more than once",
    );
}

/// The same refusal through a method the receiver's class declares.
#[test]
fn a_resource_a_method_keeps_twice_is_refused() {
    assert_compiler_error(
        &format!(
            "{ONE_RESOURCE_TWO_POSITIONS}
fn main()
    let picker = Picker(bias: 1)
    let h = Res(x: 1)
    let k = picker.pick(h, h)
"
        ),
        "more than once",
    );
}

/// Two different resources at the two kept positions are two owners handed
/// over, which the call may keep.
#[test]
fn two_resources_kept_by_one_call_are_accepted() {
    assert_heap_guard_output(
        &format!(
            "{ONE_RESOURCE_TWO_POSITIONS}
fn main()
    let h = Res(x: 1)
    let k = Res(x: 2)
    let kept = keep_both(h, k)
    println(f\"kept {{kept.x}}\")
"
        ),
        "drop 1\nkept 2\ndrop 2",
    );
}

/// Lending one resource twice to a call that keeps neither is reading it
/// twice, and the caller still owns it.
#[test]
fn a_resource_lent_twice_to_one_call_is_accepted() {
    assert_heap_guard_output(
        &format!(
            "{ONE_RESOURCE_TWO_POSITIONS}
fn main()
    let h = Res(x: 2)
    println(f\"{{read_both(h, h)}}\")
    println(\"end\")
"
        ),
        "4\nend\ndrop 2",
    );
}
