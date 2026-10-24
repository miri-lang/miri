// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A function that writes a collection, struct or tuple it was handed writes
//! a value of its own: the caller's value is unchanged, whoever else shares
//! it, and the write is seen by every later use inside the function. That
//! holds for a write on the parameter itself and for one through a path
//! starting at it — a field, a tuple element, a list element, a map entry —
//! in a function, a method, a closure, a generic function or one called
//! through a function value. An `out` parameter is the caller's binding, so a
//! write through it lands in the caller's value, which another binding may
//! still share.

use super::super::utils::*;

#[test]
fn test_a_push_on_a_parameter_leaves_the_callers_list_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn f(xs [int]) int
    xs.push(7)
    return xs.length()

fn main()
    var a = List([1, 2, 3])
    let n = f(a)
    println(f"{a.length()} {n}")
"#,
        "3 4",
    );
}

#[test]
fn test_writes_on_a_parameter_sharing_its_list_leave_both_callers_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn grow(l [int])
    l.push(1)
    l.pop()
    l.push(2)

fn main()
    var a = List([1, 2, 3])
    var b = a
    grow(b)
    println(f"a={a.length()} b={b.length()}")
"#,
        "a=3 b=3",
    );
}

#[test]
fn test_a_resize_on_a_shared_parameter_leaves_both_callers_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn grow(l [int])
    l.resize(6, 9)

fn main()
    var a = List([1, 2, 3])
    var b = a
    grow(b)
    println(f"a={a.length()} b={b.length()}")
"#,
        "a=3 b=3",
    );
}

#[test]
fn test_a_push_on_a_field_of_a_struct_parameter_leaves_the_caller_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

struct S
    items [int]

fn f(s S) int
    s.items.push(1)
    return s.items.length()

fn main()
    var a = List([1, 2])
    var s = S(items: a)
    let n = f(s)
    println(f"{s.items.length()} {a.length()} {n}")
"#,
        "2 2 3",
    );
}

#[test]
fn test_a_push_on_an_element_of_a_tuple_parameter_leaves_the_caller_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn g(t ([int], int)) int
    t.0.push(1)
    return t.0.length()

fn main()
    var a = List([1, 2])
    let t = (a, 1)
    let n = g(t)
    println(f"{t.0.length()} {a.length()} {n}")
"#,
        "2 2 3",
    );
}

#[test]
fn test_a_method_writing_its_parameter_leaves_the_caller_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class K
    var n int
    fn add(xs [int]) int
        xs.push(self.n)
        return xs.length()

fn main()
    var a = List([1, 2, 3])
    let k = K(n: 5)
    let n = k.add(a)
    var b = a
    let m = k.add(b)
    println(f"a={a.length()} b={b.length()} {n} {m}")
"#,
        "a=3 b=3 4 4",
    );
}

#[test]
fn test_a_closure_writing_its_parameter_leaves_the_caller_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    var a = List([1, 2, 3])
    let f = fn(xs [int]) int
        xs.push(7)
        return xs.length()
    var b = a
    let n = f(b)
    println(f"a={a.length()} {n}")
"#,
        "a=3 4",
    );
}

#[test]
fn test_a_generic_function_writing_its_parameter_leaves_the_caller_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn add<T>(xs [T], v T) int
    xs.push(v)
    return xs.length()

fn main()
    var a = List([1, 2, 3])
    var b = a
    let n = add(b, 4)
    println(f"a={a.length()} b={b.length()} {n}")
"#,
        "a=3 b=3 4",
    );
}

#[test]
fn test_a_function_value_writing_its_parameter_leaves_the_caller_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn add(xs [int]) int
    xs.push(1)
    return xs.length()

fn apply(g fn([int]) int, l [int]) int
    return g(l)

fn main()
    var a = List([1, 2, 3])
    var b = a
    let n = apply(add, b)
    println(f"a={a.length()} b={b.length()} {n}")
"#,
        "a=3 b=3 4",
    );
}

#[test]
fn test_a_set_on_a_map_parameter_leaves_both_callers_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn f(m Map<String, int>) int
    m.set("z", 1)
    return m.length()

fn main()
    var a = {"a": 1}
    var b = a
    let n = f(b)
    println(f"{a.length()} {b.length()} {n}")
"#,
        "1 1 2",
    );
}

/// `a = step(a)` hands `step` a list nothing else holds, so the copy `step`
/// takes is the caller's reference moved, and no write copies a list.
const STEP_AND_COUNT: &str = r#"
use system.io
use system.collections.list

fn step(xs [int]) [int]
    xs.push(1)
    return xs

fn count(xs [int]) int
    xs.push(1)
    return xs.length()

fn main()
    var a = List([1, 2, 3])
    var i = 0
    while i < 1000
        a = step(a)
        i += 1
    println(f"{a.length()}")
    var t = 0
    i = 0
    while i < 1000
        t += count(List([1, 2]))
        i += 1
    println(f"{t}")
"#;

#[test]
fn test_a_list_threaded_through_a_writing_function_is_never_copied() {
    assert_runs_with_output(STEP_AND_COUNT, "1003\n3000");
    let line = super::rc_stats::rc_stats(STEP_AND_COUNT, &[]);
    assert!(line.contains("cow_copy=0"), "{line}");
}

#[test]
fn test_a_loop_header_reads_the_parameter_the_body_writes() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn fill(xs [int]) int
    while xs.length() < 10
        xs.push(0)
    return xs.length()

fn main()
    var a = List([1, 2, 3])
    let n = fill(a)
    println(f"{a.length()} {n}")
"#,
        "3 10",
    );
}

#[test]
fn test_a_writer_on_a_shared_local_copies_the_list_once() {
    let program = r#"
use system.io
use system.collections.list

fn main()
    var a = List([1, 2, 3])
    var b = a
    b.resize(6, 9)
    println(f"{a.length()} {b.length()}")
"#;
    assert_runs_with_output(program, "3 6");
    let line = super::rc_stats::rc_stats(program, &[]);
    assert!(line.contains("cow_copy=1"), "{line}");
}

#[test]
fn test_a_push_on_an_element_of_a_list_parameter_leaves_the_caller_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn f(g [[int]]) int
    g[0].push(9)
    return g[0].length()

fn main()
    var grid = List([List([1, 2]), List([3])])
    let n = f(grid)
    println(f"{grid[0].length()} {n}")
"#,
        "2 3",
    );
}

#[test]
fn test_a_push_on_an_out_parameter_leaves_the_binding_it_shares_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn append(list out [int])
    list.push(99)

fn main()
    var a = List([1, 2])
    var b = a
    append(b)
    println(f"a={a.length()} b={b.length()}")
"#,
        "a=2 b=3",
    );
}

#[test]
fn test_a_push_through_an_out_parameter_leaves_the_list_it_shares_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

struct S
    items [int]

fn add(s out S)
    s.items.push(99)

fn main()
    var a = List([1, 2])
    var s = S(items: a)
    add(s)
    println(f"a={a.length()} s={s.items.length()}")
"#,
        "a=2 s=3",
    );
}

#[test]
fn test_a_parameter_passed_on_to_a_writing_callee_leaves_the_caller_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn inner(xs [int]) int
    xs.push(1)
    return xs.length()

fn outer(xs [int]) int
    return inner(xs)

fn main()
    var a = List([1, 2, 3])
    let n = outer(a)
    println(f"{a.length()} {n}")
"#,
        "3 4",
    );
}

#[test]
fn test_a_write_on_one_branch_leaves_the_caller_alone_on_both() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn maybe(xs [int], w bool) int
    if w
        xs.push(1)
    return xs.length()

fn main()
    var a = List([1, 2, 3])
    let n = maybe(a, true)
    let m = maybe(a, false)
    println(f"{a.length()} {n} {m}")
"#,
        "3 4 3",
    );
}

#[test]
fn test_a_recursive_function_writing_its_parameter_leaves_the_caller_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn r(xs [int], n int) int
    if n == 0
        return xs.length()
    xs.push(n)
    return r(xs, n - 1)

fn main()
    var a = List<int>()
    let k = r(a, 3)
    println(f"{a.length()} {k}")
"#,
        "0 3",
    );
}

#[test]
fn test_a_struct_parameter_holding_a_resource_runs_its_drop_hook_once() {
    let result = crate::utils::miri_run(
        r#"
use system.io
use system.collections.list

class Handle
    var id int
    fn drop(self)
        println(f"drop {self.id}")

struct S
    h Handle
    items [int]

fn f(s S) int
    s.items.push(1)
    return s.items.length()

fn main()
    var s = S(h: Handle(id: 7), items: List([1, 2]))
    let keep = s.items
    let n = f(s)
    println(f"{keep.length()} {n}")
    println("end")
"#,
    );
    assert!(result.success, "{}", result.output());
    let stdout = result.stdout.as_str();
    assert_eq!(stdout.matches("drop 7").count(), 1, "{stdout}");
    assert!(stdout.contains("2 3\nend\ndrop 7"), "{stdout}");
}

#[test]
fn test_a_push_on_a_map_entry_of_a_parameter_leaves_the_caller_alone() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list
use system.collections.map

fn f(m Map<String, [int]>) int
    m["k"].push(1)
    return m["k"].length()

fn main()
    var m = Map<String, [int]>()
    m.set("k", List([1, 2]))
    let n = f(m)
    let len = m["k"].length()
    println(f"{len} {n}")
"#,
        "2 3",
    );
}

#[test]
fn test_assigning_an_element_of_a_parameter_is_refused() {
    assert_compiler_error(
        r#"
use system.collections.list

fn f(xs [int])
    xs[0] = 99

fn main()
    var a = List([1, 2, 3])
    f(a)
"#,
        "Immutability Violation",
    );
}
