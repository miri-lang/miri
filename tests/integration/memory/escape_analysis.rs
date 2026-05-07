// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

// ─────────────────────────────────────────────────────────────────────────────
// Baseline: managed params inside function bodies must not trigger false-positive
// use-after-move errors (regression guard for Phase 12 escape analysis).
// ─────────────────────────────────────────────────────────────────────────────

// Method / self semantics
// ─────────────────────────────────────────────────────────────────────────────
// Method calls on concrete classes must not falsely consume managed receivers or
// arguments when no escape summary is present for the method. Virtual dispatch
// and inherited methods must likewise not generate false positives.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_method_receiver_not_consumed_without_summary() {
    // Calling a read-only method on a class instance multiple times must NOT
    // consume the receiver when no escape summary marks self as escaping.
    assert_runs(
        r#"
use system.io

class Square
    var side int
    fn init(s int)
        self.side = s
    fn area() int
        return self.side * self.side

fn measure_twice(sq Square)
    let a = sq.area()
    let b = sq.area()
    println(f"{a} {b}")

let sq = Square(s: 4)
measure_twice(sq)
"#,
    );
}

#[test]
fn test_method_arg_not_consumed_without_summary() {
    // Passing a managed argument to a method that only reads it must NOT
    // consume the argument variable (no escape summary for the method).
    assert_runs(
        r#"
use system.io
use system.collections.list

class Lens
    var scale int
    fn init(s int)
        self.scale = s
    fn peek(items [int]) int
        return items.length() * self.scale

fn measure_twice(lens Lens, items [int])
    let a = lens.peek(items)
    let b = lens.peek(items)
    println(f"{a} {b}")

let lens = Lens(s: 2)
let xs = List([1, 2, 3])
measure_twice(lens, xs)
"#,
    );
}

#[test]
fn test_inherited_method_receiver_not_consumed() {
    // Calling an inherited method (defined on the base class) multiple times
    // must not consume the receiver — the lookup walks the base_class chain.
    assert_runs(
        r#"
use system.io

class Base
    var x int
    fn init(v int)
        self.x = v
    fn read() int
        return self.x

class Child extends Base
    var label String
    fn init(v int, lbl String)
        super.init(v)
        self.label = lbl

fn use_child(c Child)
    let a = c.read()
    let b = c.read()
    println(f"{a} {b}")

let c = Child(v: 42, lbl: "hi")
use_child(c)
"#,
    );
}

#[test]
fn test_trait_receiver_not_consumed_without_implementer_summary() {
    // When the receiver has a trait type and no implementer escape summaries
    // are present, virtual dispatch must NOT falsely consume the receiver.
    assert_runs(
        r#"
use system.io

trait Measurable
    fn size() int

class Rect implements Measurable
    var w int
    var h int
    fn init(w int, h int)
        self.w = w
        self.h = h
    fn size() int
        return self.w * self.h

fn measure_twice(m Measurable)
    let a = m.size()
    let b = m.size()
    println(f"{a} {b}")

let r = Rect(w: 3, h: 4)
measure_twice(r)
"#,
    );
}

#[test]
fn test_method_chain_no_false_consume() {
    // Chaining multiple read-only method calls on the same receiver must not
    // consume the receiver between calls.
    assert_runs(
        r#"
use system.io

class Stats
    var min int
    var max int
    fn init(lo int, hi int)
        self.min = lo
        self.max = hi
    fn lo() int
        return self.min
    fn hi() int
        return self.max
    fn range() int
        return self.max - self.min

fn report(s Stats)
    println(f"{s.lo()} {s.hi()} {s.range()}")

let s = Stats(lo: 2, hi: 10)
report(s)
"#,
    );
}

#[test]
fn test_managed_param_passed_to_readonly_fn_no_error() {
    // Passing a list to a function that only reads it must never be flagged.
    assert_runs(
        r#"
use system.io
use system.collections.list

fn length_of(items [int]) int
    return items.length()

fn check(items [int])
    let n = length_of(items)
    let m = length_of(items)
    println(f"{n} {m}")

let xs = List([1, 2, 3])
check(xs)
"#,
    );
}

#[test]
fn test_managed_param_multi_pass_no_error() {
    // Passing the same managed param to multiple calls inside a function body
    // must not consume it.
    assert_runs(
        r#"
use system.io
use system.collections.list

fn sum(items [int]) int
    var s = 0
    var i = 0
    while i < items.length()
        s = s + items.element_at(i)
        i = i + 1
    return s

fn double_sum(items [int]) int
    let a = sum(items)
    let b = sum(items)
    return a + b

let xs = List([1, 2, 3])
println(f"{double_sum(xs)}")
"#,
    );
}

#[test]
fn test_managed_param_passed_transitively_no_error() {
    // A managed param passed through a helper chain that never stores it
    // must not be flagged.
    assert_runs(
        r#"
use system.io
use system.collections.list

fn inner(items [int]) int
    return items.length()

fn middle(items [int]) int
    return inner(items)

fn outer(items [int]) int
    return middle(items)

let xs = List([10, 20])
println(f"{outer(xs)}")
"#,
    );
}

#[test]
fn test_managed_param_recursive_no_error() {
    // A recursive function that passes the same managed param to itself
    // must not be flagged.
    assert_runs(
        r#"
use system.io
use system.collections.list

fn count_down(items [int], n int) int
    if n <= 0
        return 0
    return items.element_at(0) + count_down(items, n - 1)

let xs = List([1, 2, 3])
println(f"{count_down(xs, 3)}")
"#,
    );
}

#[test]
fn test_resource_param_inside_fn_body_still_errors() {
    // Resource types (those with fn drop(self)) must still be flagged inside
    // function bodies — Phase 12 does not change resource semantics.
    assert_compiler_error(
        r#"
use system.io

struct Conn
    host String
    fn drop(self)
        println("closed")

fn sink(c Conn)
    println(c.host)

fn use_twice(c Conn)
    sink(c)
    sink(c)

let c = Conn("db.local")
use_twice(c)
"#,
        "consumed",
    );
}

// Generics strategy
// ─────────────────────────────────────────────────────────────────────────────
// Escape analysis runs pre-monomorphization, treating generic parameters as
// typed unknowns:
//   - A managed-bounded (or unbounded) generic param `T` is analyzed exactly
//     like a concrete managed type — escape rules are structural, not nominal.
//   - A resource-bounded generic param `T extends ResourceClass` keeps the
//     strict-consume rule.
//   - Per-monomorphization re-analysis is not required: the same generic
//     function instantiated with two concrete types must not re-trigger errors.
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_unbounded_generic_param_not_consumed_in_fn_body() {
    // An unbounded generic param `T` is a managed-typed unknown — passing it
    // to a read-only helper twice inside a fn body must not be flagged.
    assert_runs(
        r#"
use system.io

fn pass_through<T>(x T)
    return

fn use_twice<T>(x T)
    pass_through(x)
    pass_through(x)

use_twice(42)
println("ok")
"#,
    );
}

#[test]
fn test_managed_bounded_generic_not_consumed_in_fn_body() {
    // A class-bounded generic where the class is NOT a resource (no fn drop)
    // must follow the managed-type pathway — no false-positive consume inside
    // a function body.
    assert_runs(
        r#"
use system.io

class Greeter
    var name String
    fn init(n String)
        self.name = n
    fn hello() String
        return self.name

fn pass_through<T extends Greeter>(x T)
    return

fn use_twice<T extends Greeter>(x T)
    pass_through(x)
    pass_through(x)

let g = Greeter(n: "world")
use_twice(g)
println("ok")
"#,
    );
}

#[test]
fn test_resource_bounded_generic_strict_consume_in_fn_body() {
    // A generic param bounded by a resource class (`fn drop(self)` defined)
    // must trigger the strict-consume rule even inside a function body.
    assert_compiler_error(
        r#"
use system.io

class Conn
    var host String
    fn init(h String)
        self.host = h
    fn drop(self)
        println("closed")

fn sink<T extends Conn>(x T)
    return

fn use_twice<T extends Conn>(x T)
    sink(x)
    sink(x)

let c = Conn(h: "db.local")
use_twice(c)
"#,
        "consumed",
    );
}

#[test]
fn test_resource_bounded_generic_assignment_is_a_move() {
    // Assignment `var b = a` where `a` has a resource-bounded generic
    // type must consume `a`.  This exercises the StatementKind::Variable arm
    // in use_after_move.rs that calls is_resource directly.
    assert_compiler_error(
        r#"
use system.io

class Conn
    var host String
    fn init(h String)
        self.host = h
    fn drop(self)
        println("closed")

fn sink<T extends Conn>(x T)
    return

fn move_then_use<T extends Conn>(a T)
    var b = a
    sink(a)

let c = Conn(h: "db.local")
move_then_use(c)
"#,
        "consumed",
    );
}

#[test]
fn test_resource_bounded_generic_method_arg_strict_consume() {
    // Resource-bounded generic passed to a method argument must strict-consume,
    // mirroring the standard behaviour for concrete resource types in method-call context.
    // The method `Logger.drain` takes a Conn directly; the caller funnels its
    // own resource-bounded generic param into it.
    assert_compiler_error(
        r#"
use system.io

class Conn
    var host String
    fn init(h String)
        self.host = h
    fn drop(self)
        println("closed")

class Logger
    fn init()
        return
    fn drain(x Conn)
        return

fn use_twice<T extends Conn>(log Logger, x T)
    log.drain(x)
    log.drain(x)

let c = Conn(h: "db.local")
let l = Logger()
use_twice(l, c)
"#,
        "consumed",
    );
}

#[test]
fn test_generic_function_two_monomorphizations_no_re_analysis() {
    // The same generic function instantiated with two different concrete
    // managed types must not re-trigger escape analysis. Monomorphization
    // specialises *types*, not the call graph — if the generic version is
    // clean, every monomorphization is clean.
    assert_runs(
        r#"
use system.io
use system.collections.list

fn pass_through<T>(x T)
    return

fn use_twice<T>(x T)
    pass_through(x)
    pass_through(x)

let xs = List([1, 2, 3])
let ys = List(["a", "b"])
use_twice(xs)
use_twice(ys)
println("ok")
"#,
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// §12.1 — Escape summaries on function definitions
//
// The escape analysis pass computes a summary for every user function that
// records which parameters escape the function (are returned, or passed to
// another escaping callee).  The use-after-move checker then uses those
// summaries inside function bodies to decide when to consume an argument.
//
// Positive tests: the pass detects escape → consumed error inside fn body.
// Negative tests: no false positives — pure-borrow functions and recursive
// functions do NOT trigger a consume.
// ─────────────────────────────────────────────────────────────────────────────

// ── Positive: single-hop escape via return expression ─────────────────────────
// A function that returns its managed parameter has direct_escapes = {0}.
// Calling it inside a function body must consume the caller's variable.

#[test]
fn test_single_hop_escape_via_return_consumes_arg() {
    assert_compiler_error(
        r#"
use system.io
use system.collections.list

fn pass_through(xs [int]) [int]
    return xs

fn check(xs [int])
    let ys = pass_through(xs)
    println(f"{ys.length()} {xs.length()}")

let data = List([1, 2, 3])
check(data)
"#,
        "consumed",
    );
}

// ── Positive: two-hop escape via return chain ─────────────────────────────────
// wrap_through calls pass_through which returns its arg; the return_aliases
// axis propagates transitively so wrap_through also escapes param 0.

#[test]
fn test_two_hop_escape_via_return_chain_consumes_arg() {
    assert_compiler_error(
        r#"
use system.io
use system.collections.list

fn pass_through(xs [int]) [int]
    return xs

fn wrap_through(xs [int]) [int]
    return pass_through(xs)

fn check(xs [int])
    let ys = wrap_through(xs)
    println(f"{ys.length()} {xs.length()}")

let data = List([1, 2, 3])
check(data)
"#,
        "consumed",
    );
}

// ── Positive: escape via non-return call (rule 4) ─────────────────────────────
// sink calls pass_through in statement position (not return); param xs still
// flows into pass_through which has direct_escapes = {0}.  The escape
// analysis detects this via the non-return call site walker.

#[test]
fn test_escape_via_non_return_call_site_consumes_arg() {
    assert_compiler_error(
        r#"
use system.io
use system.collections.list

fn pass_through(xs [int]) [int]
    return xs

fn sink(xs [int])
    let _ys = pass_through(xs)

fn check(xs [int])
    sink(xs)
    println(f"{xs.length()}")

let data = List([1, 2, 3])
check(data)
"#,
        "consumed",
    );
}

// ── Negative: pure-borrow function does not consume ────────────────────────────
// A function that only reads the list (returns int) has an empty escape
// summary; calling it must NOT consume the argument inside a function body.

#[test]
fn test_pure_borrow_summary_no_consume() {
    assert_runs(
        r#"
use system.io
use system.collections.list

fn count(xs [int]) int
    return xs.length()

fn use_twice(xs [int])
    let a = count(xs)
    let b = count(xs)
    println(f"{a} {b}")

let data = List([1, 2, 3])
use_twice(data)
"#,
    );
}

// ── Negative: recursive function with no escape ────────────────────────────────
// A self-recursive function that only reads the list does not escape its arg.
// The fixpoint must terminate with an empty summary.

#[test]
fn test_recursive_fn_no_false_positive() {
    assert_runs(
        r#"
use system.io
use system.collections.list

fn sum_n(xs [int], n int) int
    if n <= 0
        return 0
    return xs.element_at(n - 1) + sum_n(xs, n - 1)

let data = List([10, 20, 30])
let total = sum_n(data, 3)
println(f"{total}")
println(f"{data.length()}")
"#,
    );
}

// ── Negative: mutually recursive SCC with no escape ───────────────────────────
// Two functions that call each other but never store/return the managed arg.
// The SCC fixpoint must converge to empty summaries for both.

#[test]
fn test_mutual_scc_no_false_positive() {
    assert_runs(
        r#"
use system.io
use system.collections.list

fn count_from_a(xs [int], i int) int
    if i >= xs.length()
        return 0
    return xs.element_at(i) + count_from_b(xs, i + 1)

fn count_from_b(xs [int], i int) int
    if i >= xs.length()
        return 0
    return xs.element_at(i) + count_from_a(xs, i + 1)

let data = List([1, 2, 3, 4])
let a = count_from_a(data, 0)
let b = count_from_b(data, 0)
println(f"{a} {b}")
println(f"{data.length()}")
"#,
    );
}

// ── Negative: method calls on the arg do not consume (pure-borrow methods) ─────
// Calling read-only methods on a managed param inside a helper chain must not
// trigger false-positive consumed errors after escape summaries are computed.

#[test]
fn test_method_calls_on_param_no_false_consume_with_summaries() {
    assert_runs(
        r#"
use system.io
use system.collections.list

fn describe(xs [int]) int
    return xs.length()

fn report(xs [int])
    let a = describe(xs)
    let b = describe(xs)
    println(f"len={a} again={b}")

let data = List([5, 6, 7])
report(data)
"#,
    );
}

// ── Positive: escape via call inside match arm body (rule 4) ──────────────────
// pass_through returns its arg (direct_escapes = {0}).  sink_in_match passes
// xs to pass_through inside a match arm; rule 4 must therefore mark xs as
// escaping from sink_in_match.  A caller that invokes sink_in_match and then
// tries to use its argument must see a "consumed" error.

#[test]
fn test_escape_via_call_in_match_arm_consumes_arg() {
    assert_compiler_error(
        r#"
use system.io
use system.collections.list

fn pass_through(xs [int]) [int]
    return xs

fn sink_in_match(xs [int], n int)
    let _ = match n
        0: pass_through(xs).length()
        _: 0

fn caller(xs [int])
    sink_in_match(xs, 1)
    println(f"{xs.length()}")

let data = List([1, 2, 3])
caller(data)
"#,
        "consumed",
    );
}

// ── Negative: pure-borrow call inside match arm does not consume ──────────────
// count only reads xs (empty summary).  A caller that invokes inspect and then
// reads xs must NOT see a consumed error.

#[test]
fn test_pure_borrow_in_match_arm_no_consume() {
    assert_runs(
        r#"
use system.io
use system.collections.list

fn count(xs [int]) int
    return xs.length()

fn inspect(xs [int], n int)
    match n
        0: println(f"zero: {count(xs)}")
        _: println(f"other: {count(xs)}")
    println(f"still alive: {xs.length()}")

let data = List([10, 20])
inspect(data, 0)
"#,
    );
}
