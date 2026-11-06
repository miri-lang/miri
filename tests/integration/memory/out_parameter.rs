// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::super::utils::*;

// ─────────────────────────────────────────────────────
// let variable passed to out param → error
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_let_is_error() {
    assert_compiler_error(
        r#"
fn inc(x out int)
    x = x + 1

let n = 5
inc(n)
"#,
        "expected mutable variable for 'out' parameter",
    );
}

// ─────────────────────────────────────────────────────
// var variable passed to out param → ok
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_var_ok() {
    assert_type_checks(
        r#"
fn inc(x out int)
    x = x + 1

var n = 5
inc(n)
"#,
    );
}

// ─────────────────────────────────────────────────────
// non-variable expression passed to out param → error
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_literal_is_error() {
    assert_compiler_error(
        r#"
fn inc(x out int)
    x = x + 1

inc(5)
"#,
        "expected mutable variable for 'out' parameter",
    );
}

// ─────────────────────────────────────────────────────
// same variable passed twice as out → error
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_duplicate_var_is_error() {
    assert_compiler_error(
        r#"
fn swap(a out int, b out int)
    let tmp = a
    a = b
    b = tmp

var x = 1
swap(x, x)
"#,
        "same variable passed twice as 'out'",
    );
}

// ─────────────────────────────────────────────────────
// mixed: first param regular, second out — duplicate only
// for the out params, not the regular param
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_duplicate_only_out_params_tracked() {
    assert_type_checks(
        r#"
fn update(a int, b out int)
    b = a + b

var x = 1
update(x, x)
"#,
    );
}

// ─────────────────────────────────────────────────────
// type mismatch for out param → error
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_type_mismatch_is_error() {
    assert_compiler_error(
        r#"
fn inc(x out int)
    x = x + 1

var n = 5.0
inc(n)
"#,
        "Type mismatch",
    );
}

// ─────────────────────────────────────────────────────
// managed-type out param: variable must not be consumed
// after the call — the callee wrote a new value to it
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_managed_type_not_consumed_after_call() {
    assert_type_checks(
        r#"

fn append_excl(s out String)
    s = f"{s}!"

var msg = "hello"
append_excl(msg)
let result = msg
"#,
    );
}

// ─────────────────────────────────────────────────────
// resource-type out param: variable must not be consumed
// after the call, and must be usable before the call
// even if previously consumed (callee writes, not reads)
// ─────────────────────────────────────────────────────

#[test]
fn test_out_param_resource_type_not_consumed_after_call() {
    assert_type_checks(
        r#"
class Conn
    handle int
    fn drop(self)
        return

fn reset(c out Conn)
    c = Conn(handle: 99)

var c = Conn(handle: 1)
reset(c)
let x = c
"#,
    );
}

// A call writes its `out` arguments when it runs, after every argument is
// evaluated, so another argument of the same call that names the variable
// keeps the value it had before the call, whichever side of the `out`
// argument it is written on.

#[test]
fn a_list_lent_after_its_own_out_argument_keeps_its_old_value() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn bump(xs out [int], ys [int])
    xs.push(ys[0])
    println(f"ys sees {ys.length()}")

fn main()
    var a = List([1, 2, 3])
    bump(a, a)
    println(f"a is {a.length()}")
"#,
        "ys sees 3\na is 4",
    );
}

#[test]
fn a_struct_lent_after_its_own_out_argument_keeps_its_old_fields() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

struct Rec
    name String
    tags [String]

fn rename(r out Rec, s Rec)
    r.name = "new"
    r.tags.push("t")
    println(f"{s.name} {s.tags.length()}")

fn main()
    var a = Rec(name: "old", tags: List<String>())
    rename(a, a)
    println(f"{a.name} {a.tags.length()}")
"#,
        "old 0\nnew 1",
    );
}

#[test]
fn an_element_lent_beside_its_lists_out_argument_outlives_the_write() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn put(xs out [String], s String)
    xs[0] = "replaced-with-a-string-long-enough-to-allocate"
    println(s)

fn main()
    var a = List(["kept-" + "a-string-long-enough-to-allocate"])
    put(a, a[0])
    println(a[0])
"#,
        "kept-a-string-long-enough-to-allocate\nreplaced-with-a-string-long-enough-to-allocate",
    );
}

#[test]
fn a_field_lent_beside_its_values_out_argument_keeps_its_old_value() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

struct Bag
    items [String]

fn clear(b out Bag, items [String])
    b.items = List<String>()
    println(f"{items.length()} {items[0]}")

fn main()
    var a = Bag(items: List(["x"]))
    clear(a, a.items)
    println(f"{a.items.length()}")
"#,
        "1 x\n0",
    );
}

#[test]
fn a_named_argument_naming_the_out_variable_keeps_its_old_value() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn bump(xs out [int], ys [int])
    xs.push(ys[0])
    println(f"ys sees {ys.length()}")

fn main()
    var a = List([1, 2, 3])
    bump(ys: a, xs: a)
    bump(xs: a, ys: a)
    println(f"a is {a.length()}")
"#,
        "ys sees 3\nys sees 4\na is 5",
    );
}

#[test]
fn distinct_variables_passed_out_and_lent_keep_value_semantics() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn bump(xs out [int], ys [int])
    xs.push(ys[0])
    println(f"{ys.length()}")

fn main()
    var a = List([1, 2, 3])
    let b = a
    bump(a, b)
    println(f"{a.length()} {b.length()}")
"#,
        "3\n4 3",
    );
}

#[test]
fn a_scalar_read_of_the_out_variable_is_passed_as_it_was() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn grow(xs out [int], n int)
    xs.push(n)

fn main()
    var a = List([7])
    grow(a, a[0])
    grow(a, a.length())
    println(f"{a.length()} {a[1]} {a[2]}")
"#,
        "3 7 2",
    );
}
