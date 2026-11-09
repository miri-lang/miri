// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A closure holds copies of what it captures, so a write to a captured value
//! would be lost when the call ends, and is refused where it is written. A
//! write through a captured reference persists and is allowed.

use super::super::utils::*;
use crate::utils::miri_check;

const LOST_WRITE: &str = "MER_TYP_090";

#[test]
fn test_assigning_a_captured_binding_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    var n = 0
    let f = fn() int
        n = 1
        return n
    println(f"{f()}")
"#,
        LOST_WRITE,
    );
}

#[test]
fn test_compound_assigning_a_captured_binding_is_refused_and_says_the_write_is_lost() {
    let source = r#"
fn make_counter() fn() int
    var n = 0
    return fn() int:
        n += 1
        return n

fn main()
    let c = make_counter()
    println(f"{c()}")
"#;
    assert_compiler_error(source, LOST_WRITE);
    assert_compiler_error(source, "lost when the call ends");
    assert_compiler_error(source, "hold the state in a class");
}

#[test]
fn test_an_index_write_into_a_captured_list_is_refused() {
    assert_compiler_error(
        r#"
use system.collections.list

fn main()
    var xs = List([1, 2])
    let f = fn()
        xs[0] = 9
        return
    f()
"#,
        LOST_WRITE,
    );
}

#[test]
fn test_a_field_write_into_a_captured_struct_is_refused() {
    assert_compiler_error(
        r#"
struct P
    x int

fn main()
    var p = P(x: 1)
    let f = fn()
        p.x = 2
        return
    f()
"#,
        LOST_WRITE,
    );
}

#[test]
fn test_a_write_to_a_struct_field_holding_an_object_is_refused() {
    assert_compiler_error(
        r#"
class Box
    var v int

struct P
    b Box

fn main()
    var p = P(b: Box(v: 1))
    let f = fn()
        p.b = Box(v: 2)
        return
    f()
"#,
        LOST_WRITE,
    );
}

#[test]
fn test_a_writing_method_on_a_captured_list_is_refused() {
    assert_compiler_error(
        r#"
use system.collections.list

fn main()
    var xs = List([1])
    let f = fn()
        xs.push(2)
        return
    f()
"#,
        LOST_WRITE,
    );
}

#[test]
fn test_a_writing_method_on_a_list_held_by_an_immutable_binding_is_refused() {
    assert_compiler_error(
        r#"
use system.collections.list

fn main()
    let xs = List([1])
    let f = fn()
        xs.push(2)
        return
    f()
"#,
        LOST_WRITE,
    );
}

#[test]
fn test_a_captured_value_passed_as_an_out_argument_is_refused() {
    assert_compiler_error(
        r#"
fn fill(x out int)
    x = 5

fn main()
    var n = 0
    let f = fn() int
        fill(n)
        return n
    println(f"{f()}")
"#,
        LOST_WRITE,
    );
}

#[test]
fn test_a_nested_function_writing_a_captured_binding_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    var n = 0
    fn bump() int
        n += 1
        return n
    println(f"{bump()}")
"#,
        LOST_WRITE,
    );
}

#[test]
fn test_a_capture_reached_through_a_nested_closure_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    var n = 0
    let outer = fn() int
        let inner = fn() int
            n += 1
            return n
        return inner()
    println(f"{outer()}")
"#,
        LOST_WRITE,
    );
}

#[test]
fn test_a_closure_writing_a_local_of_the_closure_around_it_is_refused() {
    assert_compiler_error(
        r#"
fn main()
    let outer = fn() int
        var n = 0
        let inner = fn() int
            n += 1
            return n
        return inner()
    println(f"{outer()}")
"#,
        LOST_WRITE,
    );
}

#[test]
fn test_a_script_level_binding_written_by_a_closure_is_refused() {
    assert_compiler_error(
        r#"
var n = 0
let f = fn() int
    n += 1
    return n
println(f"{f()}")
"#,
        LOST_WRITE,
    );
}

#[test]
fn test_assigning_a_captured_object_binding_itself_is_refused() {
    assert_compiler_error(
        r#"
class Box
    var v int

fn main()
    var b = Box(v: 1)
    let f = fn()
        b = Box(v: 2)
        return
    f()
"#,
        LOST_WRITE,
    );
}

#[test]
fn test_a_write_to_an_immutable_capture_is_reported_as_immutable_only() {
    let output = miri_check(
        r#"
fn main()
    let n = 0
    let f = fn() int
        n = 1
        return n
    println(f"{f()}")
"#,
    )
    .output();
    assert!(output.contains("MER_TYP_042"), "{output}");
    assert!(!output.contains(LOST_WRITE), "{output}");
}

#[test]
fn test_reading_captures_is_allowed() {
    assert_runs_with_output(
        r#"
use system.collections.list

struct P
    x int

fn main()
    var n = 40
    var xs = List([1, 2])
    var p = P(x: 1)
    let f = fn() int: n + xs[1] + xs.length() + p.x
    println(f"{f()}")
"#,
        "45",
    );
}

#[test]
fn test_a_closures_own_parameter_and_local_may_be_written() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let f = fn(a int) int
        var b = a * 2
        b += 1
        var xs = List([b])
        xs.push(a)
        xs[0] = xs[0] + 1
        return xs[0] + xs[1] - b
    println(f"{f(1)} {f(2)}")
"#,
        "2 3",
    );
}

#[test]
fn test_a_local_or_parameter_shadowing_a_captured_name_may_be_written() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var n = 1
    let own = fn() int
        var n = 10
        n += 1
        return n
    let param = fn(xs List<int>) int
        xs.push(1)
        return xs.length()
    var xs = List([5])
    println(f"{own()} {param(xs.clone())} {xs.length()} {n}")
"#,
        "11 2 1 1",
    );
}

#[test]
fn test_a_write_through_a_captured_class_instance_persists() {
    assert_runs_with_output(
        r#"
class Counter
    var count int

fn main()
    var c = Counter(count: 0)
    let bump = fn() int
        c.count += 1
        return c.count
    println(f"{bump()} {bump()} {c.count}")
"#,
        "1 2 2",
    );
}

#[test]
fn test_a_write_through_a_captured_trait_value_persists() {
    assert_runs_with_output(
        r#"
trait Bumps
    fn bump() int

class Counter implements Bumps
    var count int
    fn bump() int
        self.count += 1
        return self.count

fn main()
    let b Bumps = Counter(count: 0)
    let f = fn() int: b.bump()
    println(f"{f()} {f()}")
"#,
        "1 2",
    );
}

#[test]
fn test_a_list_inside_a_captured_object_may_be_written() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Log
    var items List<int>

fn main()
    var log = Log(items: List([1]))
    let f = fn() int
        log.items.push(2)
        log.items[0] = 9
        return log.items.length()
    println(f"{f()} {f()} {log.items[0]} {log.items[2]}")
"#,
        "2 3 9 2",
    );
}

#[test]
fn test_a_list_element_object_written_through_a_captured_list_persists() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Box
    var v int

fn main()
    var xs = List([Box(v: 1)])
    let f = fn()
        xs[0].v = 7
        return
    f()
    println(f"{xs[0].v}")
"#,
        "7",
    );
}

#[test]
fn test_a_captured_self_writes_the_instance() {
    assert_runs_with_output(
        r#"
class K
    var c int
    fn go() int
        let g = fn() int
            self.c += 1
            return self.c
        g()
        return g()

fn main()
    let k = K(c: 0)
    println(f"{k.go()} {k.c}")
"#,
        "2 2",
    );
}

#[test]
fn test_a_function_parameter_or_returned_closure_reading_captures_is_unaffected() {
    assert_runs_with_output(
        r#"
fn apply(f fn(int) int, v int) int
    return f(v)

fn adder(n int) fn(int) int
    let base = n * 2
    return fn(v int) int: base + v

fn main()
    let add = adder(5)
    println(f"{apply(add, 1)} {add(2)}")
"#,
        "11 12",
    );
}

#[test]
fn test_a_lost_write_in_an_imported_module_points_at_that_module() {
    let files: &[(&str, &str)] = &[
        (
            "main.mi",
            "use local.helpers\nfn main()\n    println(f\"{run()}\")\n",
        ),
        (
            "helpers.mi",
            "fn run() int\n    var n = 0\n    let f = fn() int\n        n += 1\n        return n\n    return f()\n",
        ),
    ];
    assert_project_compiler_error(files, LOST_WRITE);
    assert_project_compiler_error(files, "helpers.mi");
}
