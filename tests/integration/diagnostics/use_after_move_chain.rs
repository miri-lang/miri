// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Integration tests for escape-chain diagnostics.
//
// Each test verifies that the "consumed because:" chain in use-after-move errors
// names the actual escape sink — not just the immediate callee. The chain
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
//   - Higher-order function (broad-consume fires at top level)
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
use system.collections.list

fn sink(xs [int]) [int]
    return xs

fn save(xs [int]) [int]
    return sink(xs)

let items = List([1, 2, 3])
save(items)
println(f"{items.length()}")
"#,
        "save \u{2192} calls sink (passes its argument)",
    );
}

#[test]
fn test_chain_two_hop() {
    assert_compiler_error(
        r#"
use system.collections.list

fn sink(xs [int]) [int]
    return xs

fn persist(xs [int]) [int]
    return sink(xs)

fn save(xs [int]) [int]
    return persist(xs)

let items = List([1, 2, 3])
save(items)
println(f"{items.length()}")
"#,
        "persist \u{2192} calls sink (passes its argument)",
    );
}

#[test]
fn test_chain_three_hop_with_branch() {
    assert_compiler_error(
        r#"
use system.collections.list

fn sink(xs [int]) [int]
    return xs

fn archive(xs [int]) [int]
    return sink(xs)

fn handle(xs [int]) [int]
    if xs.length() > 1
        return archive(xs)
    return xs

let items = List([1, 2, 3])
handle(items)
println(f"{items.length()}")
"#,
        "handle \u{2192} calls archive (passes its argument)",
    );
}

#[test]
fn test_chain_closure_capture() {
    assert_compiler_error(
        r#"
use system.collections.list

fn make_handler(items [int]) fn() int
    return fn() int: items.length()

let xs = List([1, 2, 3])
let cb = make_handler(xs)
println(f"{xs.length()}")
"#,
        "captures its argument in a returned closure (escape sink)",
    );
}

#[test]
fn test_chain_field_store() {
    assert_compiler_error(
        r#"
use system.collections.list

class Cache
    var data [int]
    fn init()
        self.data = List<int>()
    fn store(items [int])
        self.data = items

let c = Cache()
let xs = List([1, 2, 3])
c.store(xs)
println(f"{xs.length()}")
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
use system.collections.list

fn wrap(xs [int]) ([int], int)
    return (xs, xs.length())

let items = List([1, 2, 3])
wrap(items)
println(f"{items.length()}")
"#,
        "returns its argument in an aggregate (escape sink)",
    );
}

#[test]
fn test_chain_higher_order_fn() {
    // At top level the broad-consume rule fires: any managed-type arg is consumed
    // when passed to any function, regardless of escape summary.  The important
    // correctness claim here is that the error IS emitted — the chain detail for
    // the fn-typed-param path is absent because apply's own escape summary is
    // empty (fn-param call sites are conservatively approximated at top level).
    assert_compiler_error(
        r#"
use system.collections.list

fn sink(xs [int]) [int]
    return xs

fn apply(items [int], f fn([int]) [int]) [int]
    return f(items)

let xs = List([1, 2, 3])
apply(xs, sink)
println(f"{xs.length()}")
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
use system.collections.list

fn sink(xs [int]) [int]
    return xs

fn count_items(xs [int]) int
    if xs.length() == 0
        return 0
    return count_items(xs)

fn save_all(xs [int]) [int]
    return sink(xs)

let items = List([1, 2, 3])
save_all(items)
println(f"{items.length()}")
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
use system.collections.list

fn sink(xs [int]) [int]
    return xs

fn f(xs [int]) [int]
    return g(xs)

fn g(xs [int]) [int]
    return f(xs)

fn h(xs [int]) [int]
    return sink(xs)

fn save(xs [int]) [int]
    if xs.length() > 0
        return h(xs)
    return f(xs)

let items = List([1, 2, 3])
save(items)
println(f"{items.length()}")
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
use system.collections.list

fn write_all(filename String, xs [int]) [int]
    return xs

fn store(xs [int]) [int]
    return write_all("file.dat", xs)

let items = List([1, 2, 3])
store(items)
println(f"{items.length()}")
"#,
        "store \u{2192} calls write_all (passes its argument)",
    );
}

#[test]
fn test_chain_no_false_positive_pure_borrow() {
    // All functions only read the list — nothing stores or returns it.
    // The program must compile cleanly with no use-after-move error.
    assert_runs(
        r#"
use system.collections.list

fn print_first(items [int])
    println(f"{items.length()}")

fn print_all(items [int])
    var i = 0
    while i < items.length()
        println(f"{items.element_at(i)}")
        i = i + 1
    print_first(items)

let xs = List([1, 2, 3])
print_all(xs)
println(f"{xs.length()}")
"#,
    );
}

#[test]
fn test_chain_no_false_positive_local_mutation() {
    // count_unique creates a local Set and mutates it, but items itself
    // never escapes — it only has its elements read.  Must compile cleanly.
    assert_runs(
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

let xs = List([1, 2, 2, 3])
let n = count_unique(xs)
println(f"{xs.length()}")
println(f"{n}")
"#,
    );
}

#[test]
fn test_chain_stability_after_rename() {
    // Same two-hop scenario as 12.2.2 but with the intermediate function
    // renamed from 'persist' to 'flush_to_db'.  The chain must reflect the
    // new name — not silently degrade or keep a stale name.
    assert_compiler_error(
        r#"
use system.collections.list

fn sink(xs [int]) [int]
    return xs

fn flush_to_db(xs [int]) [int]
    return sink(xs)

fn save(xs [int]) [int]
    return flush_to_db(xs)

let items = List([1, 2, 3])
save(items)
println(f"{items.length()}")
"#,
        "flush_to_db \u{2192} calls sink (passes its argument)",
    );
}

#[test]
fn test_chain_dynamic_fn_callee() {
    // When the callee is a let-bound dynamic fn-value (not a literal function
    // name), every managed-typed argument is conservatively consumed.
    // The "consumed by" message must contain "dynamic fn" to identify
    // the dynamic dispatch as the source of the consume.
    assert_compiler_error(
        r#"
use system.collections.list

fn save(xs [int]) [int]
    return xs

fn noop(xs [int]) [int]
    return xs

let xs = List([1, 2, 3])
let cond = true
let target = if cond: save else: noop
target(xs)
println(f"{xs.length()}")
"#,
        "dynamic fn 'target'",
    );
}

#[test]
fn test_chain_inherited_field_store_walks_to_base_class() {
    // Child inherits `store` from Base. Calling `c.store(xs)` on a Child instance
    // must (a) consume `xs` (the inherited summary is found through the base_class
    // walk) and (b) render the chain naming the **base class** as the sink — the
    // method is defined on Base, not Child, so the field-store sink is Base_store.
    assert_compiler_error(
        r#"
use system.collections.list

class Base
    var data [int]
    fn init()
        self.data = List<int>()
    fn store(items [int])
        self.data = items

class Child extends Base
    var label String
    fn init(lbl String)
        super.init()
        self.label = lbl

let c = Child(lbl: "hi")
let xs = List([1, 2, 3])
c.store(xs)
println(f"{xs.length()}")
"#,
        "Base_store",
    );
}
