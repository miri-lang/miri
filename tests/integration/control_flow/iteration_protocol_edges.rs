// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Edges of `for` loops that walk an iterator: nested loops over one value,
//! closures capturing loop variables, optional map values, writes during the
//! walk, iterator drop hooks, and an `iterator()` that hands out more than one
//! class. Set and map order is unspecified, so these tests assert sums,
//! counts and sorted output.

use super::utils::*;

#[test]
fn nested_loops_over_one_set_leave_by_continue_break_and_return() {
    assert_heap_guard_output(
        r##"
use system.collections.set

fn find(s {String}, want String) String
    for x in s
        for y in s
            if y == want: return x + y
    return "none"

fn main()
    var s = Set<String>()
    s.add("a" + "b")
    s.add("c" + "d")
    var total = 0
    for x in s
        for y in s
            if y == "cd": continue
            if x == "cd": break
            total += x.length() + y.length()
    println(f"{total}")
    let found = find(s, "cd")
    println(f"{found.length()}")
    println(find(s, "zz"))
"##,
        "4\n4\nnone",
    );
}

#[test]
fn a_closure_keeps_the_key_and_value_of_a_two_variable_map_loop() {
    assert_heap_guard_output(
        r##"
use system.collections.map
use system.collections.list

fn main()
    let m = {"k" + "1": "v" + "1", "k" + "2": "v" + "2"}
    var fs = List<fn() String>()
    for k, v in m
        fs.push(fn() String: k + v)
    var out = List<String>()
    for f in fs
        out.push(f())
    out.sort()
    println(out[0] + out[1])
"##,
        "k1v1k2v2",
    );
}

#[test]
fn a_map_value_of_none_is_a_value_not_the_end_of_the_walk() {
    assert_heap_guard_output(
        r##"
use system.collections.map

fn main()
    let m = {"a" + "b": Some("x" + "y"), "c": None}
    var length = 0
    var absent = 0
    for k, v in m
        match v
            Some(t): length += t.length()
            None: absent += 1
    println(f"{length} {absent}")
"##,
        "2 1",
    );
}

#[test]
fn writing_to_a_map_inside_its_own_loop_walks_the_original() {
    assert_heap_guard_output(
        r##"
use system.collections.map

fn main()
    var m = {"a" + "a": 1, "b" + "b": 2}
    var passes = 0
    for k, v in m
        m[k + "x"] = v
        passes += 1
    println(f"{passes} {m.length()}")
    var gone = {1: "x" + "y", 2: "z" + "w"}
    for k in gone
        gone.remove(k)
    println(f"{gone.length()}")
"##,
        "2 4\n0",
    );
}

#[test]
fn breaking_out_of_a_loop_runs_the_iterator_drop_hook_once() {
    assert_heap_guard_output(
        r##"
use system.ops

class Hooked implements Iterator<int>
    var n int
    fn init()
        self.n = 0
    public fn next() int?
        if self.n >= 3: return None
        self.n += 1
        return Some(self.n)
    fn drop(self)
        println("dropped")

class Counted implements Iterable<int>
    public fn iterator() Iterator<int>
        return Hooked()

fn main()
    for i in Counted()
        if i == 2: break
        println(f"{i}")
    println("after")
"##,
        "1\ndropped\nafter",
    );
}

/// An iterable whose `iterator()` hands out one of two classes.
const EITHER: &str = r##"
use system.ops

class Either implements Iterable<String>
    var pick bool
    fn init(pick bool)
        self.pick = pick
    public fn iterator() Iterator<String>
        if self.pick: return Left()
        return Right()

class Left implements Iterator<String>, Entries<int>
    var done bool
    fn init()
        self.done = false
    public fn next() String?
        if self.done: return None
        self.done = true
        return Some("l" + "eft")
    public fn value() int?
        return Some(1)

class Right implements Iterator<String>, Entries<int>
    var done bool
    fn init()
        self.done = false
    public fn next() String?
        if self.done: return None
        self.done = true
        return Some("r" + "ight")
    public fn value() int?
        return Some(2)
"##;

#[test]
fn an_iterator_of_either_class_is_walked_through_the_trait() {
    assert_heap_guard_output(
        &format!(
            "{EITHER}\n{}",
            r##"
fn main()
    for s in Either(true)
        println(s)
    for s in Either(false)
        println(s)
"##
        ),
        "left\nright",
    );
}

#[test]
fn a_two_variable_loop_over_an_iterator_of_either_class_is_refused() {
    assert_compiler_error(
        &format!(
            "{EITHER}\n{}",
            r##"
fn main()
    for s, v in Either(true)
        println(s)
"##
        ),
        "has no value to give a second loop variable",
    );
}
