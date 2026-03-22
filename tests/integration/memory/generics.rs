// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Memory-correctness tests for generic type parameters with descriptive names.

use super::super::utils::*;

// ---------------------------------------------------------------------------
// A. Descriptive single-param generic functions with primitive arguments
// ---------------------------------------------------------------------------

#[test]
fn test_generic_descriptive_name_with_int_no_leak() {
    // "Element" is not in the old hardcoded exclusion list; primitives are
    // never heap-allocated so this must produce no IncRef/DecRef at all.
    assert_runs_with_output(
        r#"
use system.io

fn identity<Element>(e Element) Element
    e

fn main()
    let x = identity(42)
    println(f"{x}")
"#,
        "42",
    );
}

#[test]
fn test_generic_item_name_with_bool_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

fn pass<Item>(i Item) Item
    i

fn main()
    println(f"{pass(true)}")
"#,
        "true",
    );
}

#[test]
fn test_generic_value_name_with_float_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

fn wrap<Value>(v Value) Value
    v

fn main()
    println(f"{wrap(3)}")
"#,
        "3",
    );
}

// ---------------------------------------------------------------------------
// B. Descriptive single-param generic functions with managed (String) arguments
// ---------------------------------------------------------------------------

#[test]
fn test_generic_element_name_with_string_no_leak() {
    // String is a managed type — Perceus must IncRef/DecRef it correctly even
    // though the generic param is named "Element" (not in the old hardcoded list).
    assert_runs_with_output(
        r#"
use system.io

fn identity<Element>(e Element) Element
    e

fn main()
    let s = identity("hello")
    println(s)
"#,
        "hello",
    );
}

#[test]
fn test_generic_payload_name_with_string_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

fn relay<Payload>(p Payload) Payload
    p

fn main()
    let a = relay("first")
    let b = relay("second")
    println(a)
    println(b)
"#,
        "first\nsecond",
    );
}

#[test]
fn test_generic_data_name_string_through_chain_no_leak() {
    // Two levels of descriptive-named generics with a String value.
    assert_runs_with_output(
        r#"
use system.io

fn outer<Data>(d Data) Data
    inner(d)

fn inner<Data>(d Data) Data
    d

fn main()
    let s = outer("chained")
    println(s)
"#,
        "chained",
    );
}

// ---------------------------------------------------------------------------
// C. Descriptive single-param generic functions with List arguments
// ---------------------------------------------------------------------------

#[test]
fn test_generic_element_name_with_list_no_leak() {
    // List is heap-allocated — RC must be balanced despite the descriptive
    // generic name "Collection".
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn passthrough<Collection>(c Collection) Collection
    c

fn main()
    let l = List([1, 2, 3])
    let r = passthrough(l)
    println(f"{r.length()}")
"#,
        "3",
    );
}

#[test]
fn test_generic_container_name_list_used_after_call_no_leak() {
    // Caller keeps an alias; callee's copy must DecRef at function exit.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn inspect<Container>(c Container) int
    42

fn main()
    let l = List([10, 20, 30])
    let _ = inspect(l)
    println(f"{l.length()}")
"#,
        "3",
    );
}

#[test]
fn test_generic_sequence_name_list_returned_no_leak() {
    // List passed to a descriptive-named generic and then used — must not leak.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn identity<Sequence>(s Sequence) Sequence
    s

fn main()
    let l = List([7, 8, 9])
    let r = identity(l)
    println(f"{r.length()}")
"#,
        "3",
    );
}

#[test]
fn test_generic_descriptive_list_multiple_calls_no_leak() {
    // Multiple calls with different lists — each must be independently RC'd.
    // (Unconstrained generic can't call methods, so we pass-through and measure
    // the length via the original binding after the call.)
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn passthrough<Collection>(c Collection) Collection
    c

fn main()
    let a = List([1, 2])
    let b = List([3, 4, 5])
    let ra = passthrough(a)
    let rb = passthrough(b)
    println(f"{ra.length()}")
    println(f"{rb.length()}")
"#,
        "2\n3",
    );
}

// ---------------------------------------------------------------------------
// D. Descriptive single-param generic functions with class arguments
// ---------------------------------------------------------------------------

#[test]
fn test_generic_node_name_with_class_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

class Point
    var x int
    var y int

fn project<Node>(n Node) int
    42

fn main()
    let p = Point(x: 3, y: 4)
    let _ = project(p)
    println(f"{p.x}")
"#,
        "3",
    );
}

#[test]
fn test_generic_record_name_with_class_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

class Box
    var value int

fn identity<Record>(r Record) Record
    r

fn main()
    let b = Box(value: 99)
    let c = identity(b)
    println(f"{c.value}")
"#,
        "99",
    );
}

#[test]
fn test_generic_entry_name_class_with_managed_field_no_leak() {
    // Class that itself holds a managed List field — the whole graph must be RC'd.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Container
    var items [int]

fn passthrough<Entry>(e Entry) Entry
    e

fn main()
    let c = Container(items: List([1, 2, 3]))
    let r = passthrough(c)
    println(f"{r.items.length()}")
"#,
        "3",
    );
}

// ---------------------------------------------------------------------------
// E. Multi-param descriptive generics
// ---------------------------------------------------------------------------

#[test]
fn test_generic_two_descriptive_params_primitives_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

fn combine<First, Second>(a First, b Second) int
    42

fn main()
    println(f"{combine(1, true)}")
"#,
        "42",
    );
}

#[test]
fn test_generic_key_value_with_strings_no_leak() {
    // "Key" and "Value" — only "V" was in the old hardcoded list; "Key" was not.
    assert_runs_with_output(
        r#"
use system.io

fn make_label<Key, Value>(k Key, v Value) String
    "ok"

fn main()
    let label = make_label("x", "y")
    println(label)
"#,
        "ok",
    );
}

#[test]
fn test_generic_input_output_descriptive_names_no_leak() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn transform<Input, Output>(src Input, default_out Output) Output
    default_out

fn main()
    let l = List([1, 2, 3])
    let result = transform(l, "done")
    println(result)
"#,
        "done",
    );
}

// ---------------------------------------------------------------------------
// F. Generic structs with descriptive type param names
// ---------------------------------------------------------------------------

#[test]
fn test_generic_struct_element_field_int_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

struct Box<Element>
    value Element

fn main()
    let b = Box<int>(value: 42)
    println(f"{b.value}")
"#,
        "42",
    );
}

#[test]
fn test_generic_struct_item_field_string_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

struct Holder<Item>
    content Item

fn main()
    let h = Holder<String>(content: "test")
    println(h.content)
"#,
        "test",
    );
}

#[test]
fn test_generic_struct_payload_field_two_instantiations_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

struct Wrapper<Payload>
    data Payload

fn main()
    let wi = Wrapper<int>(data: 7)
    let ws = Wrapper<String>(data: "hello")
    println(f"{wi.data}")
    println(ws.data)
"#,
        "7\nhello",
    );
}

#[test]
fn test_generic_struct_container_with_list_field_no_leak() {
    // A non-generic struct with a List field, accessed through a descriptive-named
    // generic passthrough — exercises the type_params path for the struct's field.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Batch
    var items [int]

fn relay<Container>(c Container) Container
    c

fn main()
    let b = Batch(items: List([1, 2, 3]))
    let r = relay(b)
    println(f"{r.items.length()}")
"#,
        "3",
    );
}

// ---------------------------------------------------------------------------
// G. Recursive generic functions
// ---------------------------------------------------------------------------

#[test]
fn test_generic_recursive_descriptive_name_no_leak() {
    // Each recursive call must correctly manage RC for the managed argument.
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn count_down<Collection>(c Collection, n int) int
    if n <= 0
        return 0
    count_down(c, n - 1) + 1

fn main()
    let l = List([0])
    let result = count_down(l, 5)
    println(f"{result}")
"#,
        "5",
    );
}

// ---------------------------------------------------------------------------
// H. Descriptive names alongside managed return values
// ---------------------------------------------------------------------------

#[test]
fn test_generic_element_returns_string_no_leak() {
    // Generic passes through a String — must not produce an extra IncRef
    // from mistaking "Element" for a concrete custom type.
    assert_runs_with_output(
        r#"
use system.io

fn echo<Element>(e Element) Element
    e

fn main()
    let s1 = echo("alpha")
    let s2 = echo("beta")
    println(s1)
    println(s2)
"#,
        "alpha\nbeta",
    );
}

#[test]
fn test_generic_forwarded_string_used_after_return_no_leak() {
    assert_runs_with_output(
        r#"
use system.io

fn forward<Msg>(m Msg) Msg
    m

fn main()
    let msg = "original"
    let forwarded = forward(msg)
    println(forwarded)
    println(msg)
"#,
        "original\noriginal",
    );
}
