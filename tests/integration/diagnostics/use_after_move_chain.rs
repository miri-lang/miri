// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Integration tests for escape-chain diagnostics.
//
// Each test verifies that the "consumed because:" chain in use-after-move errors
// names the actual escape sink — not just the immediate callee. Only a resource
// a callee keeps is consumed, so the programs pass a `Res` with a drop hook. The chain
// follows escape_next_hops through the escape summaries computed bottom-up by
// the escape analysis pass.
//
// Coverage:
//   - Single-hop (sanity baseline)
//   - Two-hop escape chain
//   - Three-hop chain with branch (only one branch escapes)
//   - Closure-capture escape
//   - Field-store escape
//   - Aggregate-construction escape
//   - Higher-order function (a call through a function parameter keeps its argument)
//   - Recursion in call graph (fixpoint must terminate)
//   - Mutually recursive SCC where only one branch escapes
//   - Named-sink chain (multi-hop through a write helper)
//   - No false positive: pure-borrow chain
//   - No false positive: mutation of a local without escape
//   - Diagnostic stability under rename (chain updates with new name)
//   - Dynamic fn-valued callee produces "dynamic fn" in error message

use super::super::utils::*;

#[test]
fn test_chain_single_hop_baseline() {
    assert_compiler_error(
        r#"
class Res
    var x int
    fn drop(self)
        return

fn sink(xs Res) Res
    return xs

fn save(xs Res) Res
    return sink(xs)

let items = Res(x: 3)
save(items)
println(f"{items.x}")
"#,
        "save \u{2192} calls sink (passes its argument)",
    );
}

#[test]
fn test_chain_two_hop() {
    assert_compiler_error(
        r#"
class Res
    var x int
    fn drop(self)
        return

fn sink(xs Res) Res
    return xs

fn persist(xs Res) Res
    return sink(xs)

fn save(xs Res) Res
    return persist(xs)

let items = Res(x: 3)
save(items)
println(f"{items.x}")
"#,
        "persist \u{2192} calls sink (passes its argument)",
    );
}

#[test]
fn test_chain_three_hop_with_branch() {
    assert_compiler_error(
        r#"
class Res
    var x int
    fn drop(self)
        return

fn sink(xs Res) Res
    return xs

fn archive(xs Res) Res
    return sink(xs)

fn handle(xs Res) Res
    if xs.x > 1
        return archive(xs)
    return xs

let items = Res(x: 3)
handle(items)
println(f"{items.x}")
"#,
        "handle \u{2192} calls archive (passes its argument)",
    );
}

#[test]
fn test_chain_closure_capture() {
    assert_compiler_error(
        r#"
class Res
    var x int
    fn drop(self)
        return

fn make_handler(items Res) fn() int
    return fn() int: items.x

let xs = Res(x: 3)
let cb = make_handler(xs)
println(f"{xs.x}")
"#,
        "captures its argument in a returned closure (escape sink)",
    );
}

#[test]
fn test_chain_field_store() {
    assert_compiler_error(
        r#"
class Res
    var x int
    fn drop(self)
        return

class Cache
    var data Res
    fn init()
        self.data = Res(x: 0)
    fn store(items Res)
        self.data = items

let c = Cache()
let xs = Res(x: 3)
c.store(xs)
println(f"{xs.x}")
"#,
        "stores its argument into field 'data' (escape sink)",
    );
}

#[test]
fn test_chain_field_store_by_method_declaring_self() {
    // A leading `self` names the receiver, so `items` is still the first
    // argument the escape summary describes.
    assert_compiler_error(
        r#"
class Res
    var x int
    fn drop(self)
        return

class Cache
    var data Res
    fn init()
        self.data = Res(x: 0)
    fn store(self, items Res)
        self.data = items

let c = Cache()
let xs = Res(x: 3)
c.store(xs)
println(f"{xs.x}")
"#,
        "stores its argument into field 'data' (escape sink)",
    );
}

#[test]
fn test_chain_aggregate_escape() {
    // wrap bundles xs into a tuple — the escape analysis detects Tuple
    // construction as an aggregate escape and emits the "in an aggregate" chain.
    assert_compiler_error(
        r#"
class Res
    var x int
    fn drop(self)
        return

fn wrap(xs Res) (Res, int)
    return (xs, 1)

let items = Res(x: 3)
wrap(items)
println(f"{items.x}")
"#,
        "returns its argument in an aggregate (escape sink)",
    );
}

#[test]
fn test_chain_higher_order_fn() {
    // apply calls a function value its caller supplied, which it cannot see
    // into, so it may keep `items`: the resource is consumed by apply.
    assert_compiler_error(
        r#"
class Res
    var x int
    fn drop(self)
        return

fn sink(xs Res) Res
    return xs

fn apply(items Res, f fn(Res) Res) Res
    return f(items)

let xs = Res(x: 3)
apply(xs, sink)
println(f"{xs.x}")
"#,
        "was consumed by 'apply'",
    );
}

#[test]
fn test_chain_recursion_terminates() {
    // A recursive function in the call graph must not cause the fixpoint to loop.
    // The chain for save_all correctly identifies sink as the escaping function.
    assert_compiler_error(
        r#"
class Res
    var x int
    fn drop(self)
        return

fn sink(xs Res) Res
    return xs

fn count_items(xs Res) int
    if xs.x == 0
        return 0
    return count_items(xs)

fn save_all(xs Res) Res
    return sink(xs)

let items = Res(x: 3)
save_all(items)
println(f"{items.x}")
"#,
        "save_all \u{2192} calls sink (passes its argument)",
    );
}

#[test]
fn test_chain_scc_blames_correct_branch() {
    // SCC {f, g} ping-pongs without reaching a real sink → neither escapes.
    // Only the h branch (calling sink) makes save's param escape.
    // The chain must name h, NOT f or g.
    assert_compiler_error(
        r#"
class Res
    var x int
    fn drop(self)
        return

fn sink(xs Res) Res
    return xs

fn f(xs Res) Res
    return g(xs)

fn g(xs Res) Res
    return f(xs)

fn h(xs Res) Res
    return sink(xs)

fn save(xs Res) Res
    if xs.x > 0
        return h(xs)
    return f(xs)

let items = Res(x: 3)
save(items)
println(f"{items.x}")
"#,
        "save \u{2192} calls h (passes its argument)",
    );
}

#[test]
fn test_chain_named_sink() {
    // Chain through a named intermediate function (simulating a "write" sink).
    // The diagnostic must name write_all, not just store.
    assert_compiler_error(
        r#"
class Res
    var x int
    fn drop(self)
        return

fn write_all(filename String, xs Res) Res
    return xs

fn store(xs Res) Res
    return write_all("file.dat", xs)

let items = Res(x: 3)
store(items)
println(f"{items.x}")
"#,
        "store \u{2192} calls write_all (passes its argument)",
    );
}

#[test]
fn test_chain_no_false_positive_pure_borrow() {
    // All functions only read the list — nothing stores or returns it.
    // The program must compile cleanly with no use-after-move error, and every
    // borrow must still observe the intact list.
    assert_runs_with_output(
        r#"
use system.collections.list

fn print_first(items [int])
    println(f"inner_len={items.length()}")

fn print_all(items [int])
    var i = 0
    while i < items.length()
        println(f"item={items.element_at(i)}")
        i = i + 1
    print_first(items)

fn main()
    let xs = List([1, 2, 3])
    print_all(xs)
    println(f"outer_len={xs.length()}")
"#,
        "item=1\nitem=2\nitem=3\ninner_len=3\nouter_len=3",
    );
}

#[test]
fn test_chain_no_false_positive_local_mutation() {
    // count_unique creates a local Set and mutates it, but items itself
    // never escapes — it only has its elements read.  The caller's list must
    // survive the call with every element intact.
    assert_runs_with_output(
        r#"
use system.collections.list
use system.collections.set

fn count_unique(items [int]) int
    var seen = Set<int>()
    var i = 0
    while i < items.length()
        seen.add(items.element_at(i))
        i = i + 1
    return seen.length()

fn main()
    let xs = List([1, 2, 2, 3])
    let n = count_unique(xs)
    println(f"len={xs.length()}")
    println(f"unique={n}")
"#,
        "len=4\nunique=3",
    );
}

#[test]
fn test_chain_stability_after_rename() {
    // Same two-hop scenario as test_chain_two_hop but with the intermediate function
    // renamed from 'persist' to 'flush_to_db'.  The chain must reflect the
    // new name — not silently degrade or keep a stale name.
    assert_compiler_error(
        r#"
class Res
    var x int
    fn drop(self)
        return

fn sink(xs Res) Res
    return xs

fn flush_to_db(xs Res) Res
    return sink(xs)

fn save(xs Res) Res
    return flush_to_db(xs)

let items = Res(x: 3)
save(items)
println(f"{items.x}")
"#,
        "flush_to_db \u{2192} calls sink (passes its argument)",
    );
}

#[test]
fn test_chain_dynamic_fn_callee() {
    // When the callee is a let-bound dynamic fn-value (not a literal function
    // name), every resource argument is conservatively consumed.
    // The "consumed by" message must contain "dynamic fn" to identify
    // the dynamic dispatch as the source of the consume.
    assert_compiler_error(
        r#"
class Res
    var x int
    fn drop(self)
        return

fn save(xs Res) Res
    return xs

fn noop(xs Res) Res
    return xs

let xs = Res(x: 3)
let cond = true
let target = if cond: save else: noop
target(xs)
println(f"{xs.x}")
"#,
        "dynamic fn 'target'",
    );
}

#[test]
fn test_chain_inherited_field_store_walks_to_base_class() {
    // Child inherits `store` from Base. Calling `c.store(xs)` on a Child instance
    // must (a) consume `xs` (the inherited summary is found through the base_class
    // walk) and (b) render the chain naming the **base class** as the sink — the
    // method is defined on Base, not Child, so the field-store sink is Base.store.
    assert_compiler_error(
        r#"
class Res
    var x int
    fn drop(self)
        return

class Base
    var data Res
    fn init()
        self.data = Res(x: 0)
    fn store(items Res)
        self.data = items

class Child extends Base
    var label String
    fn init(lbl String)
        super.init()
        self.label = lbl

let c = Child(lbl: "hi")
let xs = Res(x: 3)
c.store(xs)
println(f"{xs.x}")
"#,
        "Base.store",
    );
}
