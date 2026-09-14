// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// A function value owned by a collection or an aggregate is released when its
// owner is.
//
// A closure is its own reference-counted allocation, so whatever holds one —
// a list element, a map value, a struct or class field, a tuple slot — has to
// release it when the holder is dropped or discards the entry. Each position is
// covered with a closure that captures a managed value, one that captures
// nothing, and a named function used as a value, since all three are closures
// by the time they are stored.

use super::super::utils::*;

/// A list of capturing closures releases each closure and its capture.
#[test]
fn test_list_of_capturing_closures_is_released() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let tag = f"n{1}="
    let up = fn(x int) String: f"{tag}{x + 1}"
    let down = fn(x int) String: f"{tag}{x - 1}"
    let fns = List([up, down])
    for f in fns
        println(f(10))
"#,
        "n1=11\nn1=9",
    );
}

/// A list of capture-free closures releases each closure.
#[test]
fn test_list_of_capture_free_closures_is_released() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let inc = fn(x int) int: x + 1
    let dec = fn(x int) int: x - 1
    let fns = List([inc, dec])
    for f in fns
        println(f"{f(10)}")
"#,
        "11\n9",
    );
}

/// A list of named function references releases each reference.
#[test]
fn test_list_of_named_functions_is_released() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn inc(x int) int
    return x + 1

fn dec(x int) int
    return x - 1

fn main()
    let fns = List([inc, dec])
    for f in fns
        println(f"{f(10)}")
"#,
        "11\n9",
    );
}

/// A map whose values are capturing closures releases each one.
#[test]
fn test_map_of_capturing_closures_is_released() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn main()
    let tag = f"t{2}:"
    var ops = Map<String, fn(x int) String>()
    ops.set("up", fn(x int) String: f"{tag}{x + 1}")
    ops.set("down", fn(x int) String: f"{tag}{x - 1}")
    match ops.get("up")
        Some(up): println(up(10))
        None: println("none")
"#,
        "t2:11",
    );
}

/// A map whose values are capture-free closures releases each one.
#[test]
fn test_map_of_capture_free_closures_is_released() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn main()
    var ops = Map<String, fn(x int) int>()
    ops.set("inc", fn(x int) int: x + 1)
    ops.set("dec", fn(x int) int: x - 1)
    match ops.get("dec")
        Some(dec): println(f"{dec(10)}")
        None: println("none")
"#,
        "9",
    );
}

/// A map whose values are named function references releases each one.
#[test]
fn test_map_of_named_functions_is_released() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn inc(x int) int
    return x + 1

fn dec(x int) int
    return x - 1

fn main()
    var ops = Map<String, fn(x int) int>()
    ops.set("inc", inc)
    ops.set("dec", dec)
    match ops.get("inc")
        Some(chosen): println(f"{chosen(10)}")
        None: println("none")
"#,
        "11",
    );
}

/// A struct field holding a capturing closure is released with the struct.
#[test]
fn test_struct_field_capturing_closure_is_released() {
    assert_heap_guard_output(
        r#"
struct Op
    run fn(x int) String

fn main()
    let tag = f"s{3}:"
    let op = Op(fn(x int) String: f"{tag}{x}")
    println(op.run(10))
"#,
        "s3:10",
    );
}

/// A struct field holding a capture-free closure is released with the struct.
#[test]
fn test_struct_field_capture_free_closure_is_released() {
    assert_heap_guard_output(
        r#"
struct Op
    run fn(x int) int

fn main()
    let op = Op(fn(x int) int: x + 1)
    println(f"{op.run(10)}")
"#,
        "11",
    );
}

/// A struct field holding a named function reference is released with the
/// struct.
#[test]
fn test_struct_field_named_function_is_released() {
    assert_heap_guard_output(
        r#"
struct Op
    run fn(x int) int

fn triple(x int) int
    return x * 3

fn main()
    let op = Op(triple)
    println(f"{op.run(10)}")
"#,
        "30",
    );
}

/// A class field holding a capturing closure is released with the instance.
#[test]
fn test_class_field_capturing_closure_is_released() {
    assert_heap_guard_output(
        r#"
class Handler
    var run fn(x int) String

fn main()
    let tag = f"c{4}:"
    let h = Handler(run: fn(x int) String: f"{tag}{x}")
    println(h.run(10))
"#,
        "c4:10",
    );
}

/// A class field holding a capture-free closure is released with the instance.
#[test]
fn test_class_field_capture_free_closure_is_released() {
    assert_heap_guard_output(
        r#"
class Handler
    var run fn(x int) int

fn main()
    let h = Handler(run: fn(x int) int: x - 1)
    println(f"{h.run(10)}")
"#,
        "9",
    );
}

/// A class field holding a named function reference is released with the
/// instance.
#[test]
fn test_class_field_named_function_is_released() {
    assert_heap_guard_output(
        r#"
class Handler
    var run fn(x int) int

fn negate(x int) int
    return 0 - x

fn main()
    let h = Handler(run: negate)
    println(f"{h.run(10)}")
"#,
        "-10",
    );
}

/// Reassigning a closure-typed class field releases the closure it replaces.
#[test]
fn test_reassigned_class_field_releases_replaced_closure() {
    assert_heap_guard_output(
        r#"
class Handler
    var run fn(x int) int

fn main()
    let step = 5
    var h = Handler(run: fn(x int) int: x)
    h.run = fn(x int) int: x + step
    h.run = fn(x int) int: x * step
    println(f"{h.run(2)}")
"#,
        "10",
    );
}

/// Every way a list discards an element releases a closure element: `pop`,
/// `remove_at`, an index overwrite, `set` and `clear`.
#[test]
fn test_list_discarding_closure_elements_releases_them() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let tag = f"v{5}:"
    var fns = List<fn(x int) String>()
    fns.push(fn(x int) String: f"{tag}{x}")
    fns.push(fn(x int) String: f"{tag}{x * 2}")
    fns.push(fn(x int) String: f"{tag}{x * 3}")
    fns.insert(0, fn(x int) String: f"i{x}")
    let last = fns.pop()
    fns[0] = fn(x int) String: f"w{x}"
    fns.set(1, fn(x int) String: f"s{x}")
    println(fns[0](5))
    println(fns[1](5))
    match fns.remove_at(2)
        Some(r): println(r(7))
        None: println("none")
    match last
        Some(l): println(l(1))
        None: println("none")
    fns.clear()
    println(f"{fns.length()}")
"#,
        "w5\ns5\nv5:14\nv5:3\n0",
    );
}

/// Overwriting and removing a map's closure values releases the discarded ones.
#[test]
fn test_map_discarding_closure_values_releases_them() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn main()
    let tag = f"m{6}:"
    var ops = Map<String, fn(x int) String>()
    ops.set("a", fn(x int) String: f"{tag}{x}")
    ops.set("a", fn(x int) String: f"{tag}{x + 1}")
    ops.set("b", fn(x int) String: f"{tag}{x + 2}")
    match ops.get("a")
        Some(g): println(g(1))
        None: println("none")
    ops.remove("b")
    println(f"{ops.length()}")
    ops.clear()
    println(f"{ops.length()}")
"#,
        "m6:2\n1\n0",
    );
}

/// A closure held in a tuple slot, an `Option` and an enum payload is released
/// with its holder.
#[test]
fn test_closure_in_tuple_option_and_enum_payload_is_released() {
    assert_heap_guard_output(
        r#"
type IntFn is fn(x int) int

enum Action
    Run(fn(x int) int)
    Stop

fn pick(flag bool) IntFn?
    if flag
        return fn(x int) int: x + 100
    return None

fn main()
    let tag = f"p{7}:"
    let pair = (fn(x int) String: f"{tag}{x}", 3)
    println(pair.0(pair.1))
    match pick(true)
        Some(f): println(f"{f(1)}")
        None: println("none")
    let k = 5
    let a = Action.Run(fn(x int) int: x + k)
    match a
        Action.Run(f): println(f"{f(1)}")
        Action.Stop: println("stop")
"#,
        "p7:3\n101\n6",
    );
}

/// A struct holding a closure, stored in a list and reached through a copy of
/// that list, is released once when the last owner goes.
#[test]
fn test_list_of_structs_with_closure_fields_is_released() {
    assert_heap_guard_output(
        r#"
use system.collections.list

struct Op
    name String
    run fn(x int) int

fn double(x int) int
    return x * 2

fn main()
    let ops = List([Op("d", double), Op("i", fn(x int) int: x + 1)])
    let copy = ops
    for op in copy
        println(f"{op.name}{op.run(10)}")
"#,
        "d20\ni11",
    );
}

/// Closures built in one function, returned inside a list, and captured by a
/// further closure are each released exactly once.
#[test]
fn test_returned_list_of_closures_captured_by_closure_is_released() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn make(n int) fn(x int) int
    let inner = fn(x int) int: x + n
    return fn(x int) int: inner(x) * 2

fn build() List<fn(x int) int>
    var out = List<fn(x int) int>()
    out.push(make(1))
    out.push(make(2))
    return out

fn total(fs List<fn(x int) int>) int
    var s = 0
    for f in fs
        s = s + f(10)
    return s

fn main()
    let fs = build()
    println(f"{total(fs)}")
    let again = fn() int: total(fs)
    println(f"{again()}")
"#,
        "46\n46",
    );
}
