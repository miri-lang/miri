// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A `let` may redeclare a name its own scope already bound. Each declaration
//! is a binding of its own: every one is released once when the scope ends,
//! on every way out of it, and leaving the scope gives the name back the
//! binding it had before the scope began — never one the scope made itself.

use super::utils::*;

#[test]
fn a_redeclared_list_element_binding_releases_each_element_once() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn main()
    let a = List([List([1, 2]), List([3, 4, 5])])
    let t = a[0]
    println(f"{t.length()}")
    let t = a[1]
    println(f"{t.length()}")
"#,
        "2\n3",
    );
}

#[test]
fn a_redeclared_string_releases_both_values() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    let s = "a" + "b"
    let s = s + "c"
    println(s)
"#,
        "abc",
    );
}

#[test]
fn a_redeclared_class_instance_drops_each_instance_once() {
    assert_heap_guard_output(
        r#"
use system.io

class Res
    var id int
    fn drop(self)
        println(f"drop {self.id}")

fn main()
    let r = Res(id: 1)
    let r = Res(id: 2)
    println(f"live {r.id}")
"#,
        "live 2\ndrop 2\ndrop 1",
    );
}

#[test]
fn an_early_return_releases_every_redeclared_binding() {
    assert_heap_guard_output(
        r#"
use system.io

fn pick(n int) int
    let s = "a" + "b"
    let s = s + "c"
    if n > 0
        return s.length()
    let s = s + "d"
    s.length()

fn main()
    println(f"{pick(1)} {pick(0)}")
"#,
        "3 4",
    );
}

#[test]
fn a_break_releases_every_redeclared_binding_of_the_loop_body() {
    assert_heap_guard_output(
        r#"
use system.io

fn main()
    var i = 0
    while true
        let s = f"{i}" + "x"
        let s = s + "y"
        if i == 2
            println(s)
            break
        i = i + 1
"#,
        "2xy",
    );
}

#[test]
fn a_parameter_redeclared_by_a_let_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.io

fn shout(s String) String
    let s = s + "!"
    s

fn main()
    let m = "hi" + ""
    println(shout(m))
    println(m)
"#,
        "hi!\nhi",
    );
}

#[test]
fn leaving_a_scope_restores_the_binding_from_before_it_began() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let t = "outer"
    if true
        let t = "a"
        let t = "b"
        println(f"[{t}]")
    println(f"[{t}]")
"#,
        "[b]\n[outer]",
    );
}
