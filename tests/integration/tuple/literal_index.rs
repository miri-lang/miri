// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A tuple indexed by an integer literal — `t[1]` — names one element at a
//! position fixed when the program is compiled, exactly as `t.1` does. The
//! element read is that position's own type, whatever the other elements
//! hold: a counted element is retained by the binding that takes it, and an
//! element of a tuple whose elements differ in width is read at its own offset.

use super::utils::*;

#[test]
fn a_list_read_out_of_a_tuple_into_a_reassigned_var_is_retained() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn pass(xs [int]) (int, [int])
    (xs.length(), xs)

var s = List<int>()
s.push(7)
let r = pass(s)
s = r[1]
println(f"{s.length()}")
s.push(1)
println(f"{s.length()}")
"#,
        "1\n2",
    );
}

#[test]
fn a_list_read_out_of_a_tuple_keeps_its_value_across_loop_iterations() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn grow(xs [int]) (int, [int])
    xs.push(xs.length())
    (xs.length(), xs)

fn main()
    var s = List<int>()
    var i = 0
    while i < 3
        let r = grow(s)
        s = r[1]
        i = i + 1
    println(f"{s.length()} {s[2]}")
"#,
        "3 2",
    );
}

#[test]
fn a_list_read_out_of_a_tuple_into_a_fresh_binding_is_retained() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn main()
    let r = (5, List([1, 2, 3]))
    let q = r[1]
    println(f"{q.length()} {r[0]}")
"#,
        "3 5",
    );
}

#[test]
fn a_string_read_out_of_a_tuple_into_a_fresh_binding_is_retained() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let r = (5, "a" + "b")
    let q = r[1]
    var s = "x" + "y"
    s = r[1]
    println(f"{q} {s} {r[0]}")
"#,
        "ab ab 5",
    );
}

#[test]
fn a_class_instance_read_out_of_a_tuple_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.io

class Res
    var id int
    fn drop(self)
        println(f"drop {self.id}")

fn main()
    let t = (0, Res(id: 7))
    let r = t[1]
    println(f"got {r.id}")
"#,
        "got 7\ndrop 7",
    );
}

#[test]
fn an_element_of_a_tuple_of_mixed_widths_is_read_at_its_own_offset() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let r = (true, 42)
    let t = (7 as i8, 1234567890123, 2.5 as f32)
    println(f"[{r[1]}] [{r[0]}] [{t[0]}] [{t[1]}] [{t[2]}]")
"#,
        "[42] [true] [7] [1234567890123] [2.5]",
    );
}
