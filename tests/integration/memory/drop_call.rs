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

/// A struct's hook prints no id: a struct hook that reads a field of `self`
/// does not compile yet, whether or not the hook is ever called directly.
fn struct_handle(main: &str) -> String {
    format!(
        r#"
struct Handle
    id int

    fn drop(self)
        println("dropped")

{main}"#
    )
}

fn both_spellings(main: &str) -> [String; 2] {
    [class_handle(main), struct_handle(main)]
}

/// Runs `main` against both spellings under the heap guard. The class hook
/// prints `dropped <id>`; the struct hook prints `dropped`, so its expected
/// output is `expected` with the ids taken off.
fn assert_both_spellings_print(main: &str, expected: &str) {
    assert_heap_guard_output(&class_handle(main), expected);
    let without_ids: Vec<&str> = expected
        .lines()
        .map(|line| {
            if line.starts_with("dropped ") {
                "dropped"
            } else {
                line
            }
        })
        .collect();
    assert_heap_guard_output(&struct_handle(main), &without_ids.join("\n"));
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
    assert_both_spellings_print(
        r#"
fn main()
    var h = Handle(id: 1)
    h.drop()
    println("end")
"#,
        "dropped 1\nend",
    );
}

#[test]
fn test_drop_call_on_let_binding_runs_the_hook_once() {
    assert_both_spellings_print(
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
    for code in both_spellings(
        r#"
fn main()
    let h = Handle(id: 1)
    h.drop()
"#,
    ) {
        assert_no_diagnostic(&code, "MER_OWN_001");
    }
}

#[test]
fn test_use_after_drop_call_is_use_of_moved_value() {
    for code in both_spellings(
        r#"
fn main()
    let h = Handle(id: 1)
    h.drop()
    println(f"{h.id}")
"#,
    ) {
        assert_compiler_error(&code, "'h' was consumed by 'drop'");
    }
}

#[test]
fn test_second_drop_call_is_use_of_moved_value() {
    for code in both_spellings(
        r#"
fn main()
    let h = Handle(id: 1)
    h.drop()
    h.drop()
"#,
    ) {
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
    assert_both_spellings_print(&program("true"), "dropped 3\nend");
    assert_both_spellings_print(&program("false"), "end\ndropped 3");
}

#[test]
fn test_drop_call_on_a_call_result_runs_the_hook_once() {
    assert_both_spellings_print(
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
    assert_both_spellings_print(
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
    assert_both_spellings_print(
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
    assert_both_spellings_print(
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
    for code in both_spellings(
        r#"
fn close(h Handle)
    h.drop()

fn main()
    let h = Handle(id: 1)
    close(h)
"#,
    ) {
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
    for code in both_spellings(
        r#"
class Holder
    public var h Handle

fn main()
    let holder = Holder(h: Handle(id: 1))
    holder.h.drop()
"#,
    ) {
        assert_compiler_error(&code, BORROWED_DROP);
    }
}

#[test]
fn test_drop_call_on_a_loop_element_is_refused() {
    for code in both_spellings(
        r#"
use system.collections.list

fn main()
    var handles = List<Handle>()
    handles.push(Handle(id: 1))
    for h in handles
        h.drop()
"#,
    ) {
        assert_compiler_error(&code, BORROWED_DROP);
    }
}

#[test]
fn test_drop_call_on_a_captured_local_is_refused() {
    for code in both_spellings(
        r#"
fn main()
    let h = Handle(id: 1)
    let close = fn()
        h.drop()
    close()
"#,
    ) {
        assert_compiler_error(&code, BORROWED_DROP);
    }
}
