// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Calling a resource's drop hook directly: `value.drop()` consumes a value the
//! scope owns, runs the hook once, and leaves nothing for the scope to release.

use super::super::utils::*;

const BORROWED_DROP: &str = "drop() can only release a value this scope owns";

fn class_handle(main: &str) -> String {
    format!(
        r#"
class Handle
    public var id int

    public fn drop(self)
        println(f"dropped {{self.id}}")

{main}"#
    )
}

/// Runs `main` against a `Handle` resource under the heap guard; its hook
/// prints `dropped <id>`.
fn assert_handle_prints(main: &str, expected: &str) {
    assert_heap_guard_output(&class_handle(main), expected);
}

fn assert_no_diagnostic(code: &str, diagnostic: &str) {
    let output = crate::utils::miri_check(code).output();
    assert!(
        !output.contains(diagnostic),
        "expected no {diagnostic}:\n{output}"
    );
}

#[test]
fn test_drop_call_runs_the_hook_once_at_the_call() {
    assert_handle_prints(
        r#"
fn main()
    var h = Handle(id: 1)
    h.drop()
    println("end")
"#,
        "dropped 1\nend",
    );
}

/// A move hands the reference on: `drop()` on the binding it was moved to is
/// the last release, so the hook runs at the call, not when the binding it
/// was moved out of leaves scope.
#[test]
fn test_drop_call_on_a_moved_local_runs_the_hook_at_the_call() {
    assert_handle_prints(
        r#"
fn main()
    var h = Handle(id: 1)
    var g = h
    var k = g
    k.drop()
    println("end")
"#,
        "dropped 1\nend",
    );
}

/// A move that happens on one path only leaves the other path's value to be
/// released at its scope exit, once.
#[test]
fn test_a_move_on_one_branch_releases_once_on_either_path() {
    assert_handle_prints(
        r#"
fn run(c bool)
    var h = Handle(id: 2)
    if c
        var g = h
        g.drop()
        println("moved")
    println("end")

fn main()
    run(true)
    run(false)
"#,
        "dropped 2\nmoved\nend\nend\ndropped 2",
    );
}

/// The binding a resource was moved out of may be given a new one, which it
/// then releases on its own.
#[test]
fn test_a_moved_out_binding_given_a_new_value_releases_only_that_value() {
    assert_handle_prints(
        r#"
fn main()
    var h = Handle(id: 3)
    var g = h
    h = Handle(id: 4)
    g.drop()
    println("mid")
"#,
        "dropped 3\nmid\ndropped 4",
    );
}

#[test]
fn test_drop_call_on_let_binding_runs_the_hook_once() {
    assert_handle_prints(
        r#"
fn main()
    let h = Handle(id: 7)
    println("start")
    h.drop()
    println("end")
"#,
        "start\ndropped 7\nend",
    );
}

#[test]
fn test_drop_call_consumes_the_resource_for_the_scope_exit_warning() {
    for code in [class_handle(
        r#"
fn main()
    let h = Handle(id: 1)
    h.drop()
"#,
    )] {
        assert_no_diagnostic(&code, "MER_OWN_001");
    }
}

#[test]
fn test_use_after_drop_call_is_use_of_moved_value() {
    for code in [class_handle(
        r#"
fn main()
    let h = Handle(id: 1)
    h.drop()
    println(f"{h.id}")
"#,
    )] {
        assert_compiler_error(&code, "'h' was consumed by 'drop'");
    }
}

#[test]
fn test_second_drop_call_is_use_of_moved_value() {
    for code in [class_handle(
        r#"
fn main()
    let h = Handle(id: 1)
    h.drop()
    h.drop()
"#,
    )] {
        assert_compiler_error(&code, "'h' was consumed by 'drop'");
    }
}

#[test]
fn test_drop_call_in_one_branch_runs_the_hook_once_on_either_path() {
    let program = |flag: &str| {
        format!(
            r#"
fn main()
    let h = Handle(id: 3)
    if {flag}
        h.drop()
    println("end")
"#
        )
    };
    assert_handle_prints(&program("true"), "dropped 3\nend");
    assert_handle_prints(&program("false"), "end\ndropped 3");
}

#[test]
fn test_drop_call_on_a_call_result_runs_the_hook_once() {
    assert_handle_prints(
        r#"
fn make(id int) Handle
    return Handle(id: id)

fn main()
    make(4).drop()
    println("end")
"#,
        "dropped 4\nend",
    );
}

#[test]
fn test_rebinding_after_drop_call_releases_only_the_new_value() {
    assert_handle_prints(
        r#"
fn main()
    var h = Handle(id: 1)
    h.drop()
    h = Handle(id: 2)
    println("end")
"#,
        "dropped 1\nend\ndropped 2",
    );
}

#[test]
fn test_drop_call_each_iteration_releases_that_iteration_value() {
    assert_handle_prints(
        r#"
fn main()
    for i in 0..3
        let h = Handle(id: i)
        h.drop()
    println("end")
"#,
        "dropped 0\ndropped 1\ndropped 2\nend",
    );
}

#[test]
fn test_local_shadowing_a_parameter_can_be_dropped_inside_its_block() {
    assert_handle_prints(
        r#"
fn run(h Handle)
    if true
        let h = Handle(id: 9)
        h.drop()
    println(f"{h.id}")

fn main()
    let h = Handle(id: 1)
    run(h)
"#,
        "dropped 9\n1\ndropped 1",
    );
}

#[test]
fn test_drop_call_on_a_parameter_is_refused() {
    for code in [class_handle(
        r#"
fn close(h Handle)
    h.drop()

fn main()
    let h = Handle(id: 1)
    close(h)
"#,
    )] {
        assert_compiler_error(&code, BORROWED_DROP);
    }
}

#[test]
fn test_drop_call_on_self_is_refused() {
    assert_compiler_error(
        r#"
class Handle
    public var id int

    public fn close()
        self.drop()

    public fn drop(self)
        println("dropped")

fn main()
    let h = Handle(id: 1)
    h.close()
"#,
        BORROWED_DROP,
    );
}

#[test]
fn test_drop_call_on_a_field_is_refused() {
    for code in [class_handle(
        r#"
class Holder
    public var h Handle

fn main()
    let holder = Holder(h: Handle(id: 1))
    holder.h.drop()
"#,
    )] {
        assert_compiler_error(&code, BORROWED_DROP);
    }
}

#[test]
fn test_drop_call_on_a_loop_element_is_refused() {
    for code in [class_handle(
        r#"
use system.collections.list

fn main()
    var handles = List<Handle>()
    handles.push(Handle(id: 1))
    for h in handles
        h.drop()
"#,
    )] {
        assert_compiler_error(&code, BORROWED_DROP);
    }
}

#[test]
fn test_drop_call_on_a_captured_local_is_refused() {
    for code in [class_handle(
        r#"
fn main()
    let h = Handle(id: 1)
    let close = fn()
        h.drop()
    close()
"#,
    )] {
        assert_compiler_error(&code, BORROWED_DROP);
    }
}

/// A trait declaring the hook, either abstract (`Handle` declares it) or with
/// a default (`Handle` runs the trait's).
fn trait_handles(main: &str) -> [(String, &'static str); 2] {
    let abstract_hook = format!(
        r#"
trait Closable
    fn drop(self)

class Handle implements Closable
    public var id int

    public fn drop(self)
        println(f"dropped {{self.id}}")

{main}"#
    );
    let default_hook = format!(
        r#"
trait Closable
    fn drop(self)
        println("dropped")

class Handle implements Closable
    public var id int

{main}"#
    );
    [(abstract_hook, "dropped 1"), (default_hook, "dropped")]
}

/// `drop()` on a local held at a trait declaring the hook releases it at the
/// call, which runs the instance's own hook once through its vtable.
#[test]
fn test_drop_call_on_a_trait_typed_local_runs_the_hook_once_at_the_call() {
    for (code, dropped) in trait_handles(
        r#"
fn main()
    var c Closable = Handle(id: 1)
    c.drop()
    println("end")
"#,
    ) {
        assert_heap_guard_output(&code, &format!("{dropped}\nend"));
    }
}

/// A trait-typed parameter is borrowed like any other, so `drop()` on it is
/// refused rather than run here and again by the caller's release.
#[test]
fn test_drop_call_on_a_trait_typed_parameter_is_refused() {
    for (code, _) in trait_handles(
        r#"
fn shut(x Closable)
    x.drop()

fn main()
    let h = Handle(id: 1)
    shut(h)
"#,
    ) {
        assert_compiler_error(&code, BORROWED_DROP);
    }
}
