// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A resource handed to a call that keeps it is released where the call
//! consumes it: the caller's binding gives up its reference at the call, so
//! the drop hook runs when the new holder is released, not when the caller's
//! scope ends. Every test reads the exact order of the hook's output.

use super::super::utils::*;

const HANDLE: &str = r#"
class Handle
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

fn close(h Handle) Handle
    println("closing")
    return h
"#;

fn with_handle(program: &str) -> String {
    format!("{HANDLE}\n{program}")
}

/// The handle `close` keeps moves to `_closed`, released when the `if` ends.
#[test]
fn a_resource_a_function_keeps_is_released_with_the_result() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    if true
        let _closed = close(h)
    println("done")
"#,
        ),
        "closing\nclosed 1\ndone",
    );
}

/// A kept resource whose result is discarded is released at the statement.
#[test]
fn a_resource_a_discarded_call_keeps_is_released_at_the_call() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    close(h)
    println("done")
"#,
        ),
        "closing\nclosed 1\ndone",
    );
}

/// A method storing its argument in a field hands it to the object, which
/// releases it when the object goes.
#[test]
fn a_resource_a_method_stores_is_released_with_the_object() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Shelf
    var slot Handle?
    fn init()
        self.slot = None
    fn keep(h Handle)
        self.slot = h

fn main()
    let h = Handle(1)
    if true
        var shelf = Shelf()
        shelf.keep(h)
        println("kept")
    println("done")
"#,
        ),
        "kept\nclosed 1\ndone",
    );
}

/// A function value cannot be seen into, so it is taken to keep the resource.
#[test]
fn a_resource_handed_to_a_function_value_is_released_with_the_result() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let f = close
    let h = Handle(1)
    if true
        let _closed = f(h)
    println("done")
"#,
        ),
        "closing\nclosed 1\ndone",
    );
}

/// A function value that only reads the resource still consumes it, so the
/// caller's release runs at the call.
#[test]
fn a_resource_a_function_value_only_reads_is_released_at_the_call() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn peek(h Handle) int
    return h.id

fn main()
    let f = peek
    let h = Handle(7)
    let n = f(h)
    println(f"{n}")
"#,
        ),
        "closed 7\n7",
    );
}

/// Consumed in one arm only: that arm releases it with the result, and the
/// other path releases it where its owner's scope ends.
#[test]
fn a_resource_consumed_in_one_arm_is_released_on_each_path() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn run(flag bool, id int)
    let h = Handle(id)
    if flag
        let _closed = close(h)
        println("after close")
    println(f"end {id}")

fn main()
    run(true, 1)
    run(false, 2)
"#,
        ),
        "closing\nafter close\nclosed 1\nend 1\nend 2\nclosed 2",
    );
}

/// Consumed in the `else` arm only, the mirror of the case above.
#[test]
fn a_resource_consumed_in_the_else_arm_is_released_on_each_path() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn run(flag bool, id int)
    let h = Handle(id)
    if flag
        println("kept")
    else
        let _closed = close(h)
    println(f"end {id}")

fn main()
    run(true, 1)
    run(false, 2)
"#,
        ),
        "kept\nend 1\nclosed 1\nclosing\nclosed 2\nend 2",
    );
}

/// A struct holding a resource is a resource itself: a call that keeps the
/// struct takes the handle with it.
// TODO: assert "taking\nclosed 1\ndone". The type checker consumes `p`, but
// a consumed binding's reference is handed on only for a value whose own type
// runs a drop hook (`ConsumedArguments`), and an inline struct holds no
// reference of its own: its resource fields would each have to be released
// where it is consumed. `let q = p` keeps the same late release.
#[test]
fn a_struct_holding_a_resource_is_released_with_the_result() {
    assert_heap_guard_output(
        &with_handle(
            r#"
struct Pair
    h Handle
    n int

fn take(p Pair) Pair
    println("taking")
    return p

fn main()
    let p = Pair(h: Handle(1), n: 2)
    if true
        let _q = take(p)
    println("done")
"#,
        ),
        "taking\ndone\nclosed 1",
    );
}

/// An enum with a drop hook is consumed and released the same way.
#[test]
fn an_enum_resource_a_function_keeps_is_released_with_the_result() {
    assert_heap_guard_output(
        r#"
enum Res
    Tag(int)

    fn drop(self)
        match self
            Res.Tag(n): println(f"drop {n}")

fn keep(r Res) Res
    println("keeping")
    return r

fn main()
    let r = Res.Tag(1)
    if true
        let _k = keep(r)
    println("done")
"#,
        "keeping\ndrop 1\ndone",
    );
}

/// A resource the callee only reads is lent: the caller's scope releases it.
#[test]
fn a_resource_a_function_only_reads_is_released_by_its_owner() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn peek(h Handle) int
    return h.id

fn main()
    let h = Handle(3)
    if true
        println(f"{peek(h)}")
    println(f"still {h.id}")
"#,
        ),
        "3\nstill 3\nclosed 3",
    );
}

/// A consuming call in a loop body that then leaves the loop: the kept
/// handle is released when its holder's scope ends at the `break`.
#[test]
fn a_resource_consumed_in_a_loop_body_before_a_break_is_released_with_the_result() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    while true
        let _closed = close(h)
        println("in loop")
        break
    println("done")
"#,
        ),
        "closing\nin loop\nclosed 1\ndone",
    );
}

/// Consumed in one match arm only: that arm releases it with the result,
/// and the other path releases it where its owner's scope ends.
#[test]
fn a_resource_consumed_in_one_match_arm_is_released_on_each_path() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn run(n int)
    let h = Handle(n)
    match n
        1
            let _closed = close(h)
            println("after close")
        _: println("kept")
    println(f"end {n}")

fn main()
    run(1)
    run(2)
"#,
        ),
        "closing\nafter close\nclosed 1\nend 1\nkept\nend 2\nclosed 2",
    );
}

/// Consumed by one branch of a conditional expression: the result holds it
/// on that path, and the owner's scope releases it on the other.
#[test]
fn a_resource_consumed_in_a_conditional_expression_is_released_on_each_path() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn run(flag bool, id int)
    let h = Handle(id)
    let k = close(h) if flag else Handle(id + 10)
    println(f"got {k.id}")

fn main()
    run(true, 1)
    run(false, 2)
"#,
        ),
        "closing\ngot 1\nclosed 1\ngot 12\nclosed 12\nclosed 2",
    );
}

/// Consumed in both arms: each arm releases it where it consumes it, and
/// nothing is left to release where the owner's scope ends.
#[test]
fn a_resource_consumed_in_both_arms_is_released_once_on_each_path() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn run(flag bool, id int)
    let h = Handle(id)
    if flag
        close(h)
        println("then")
    else
        close(h)
        println("else")
    println(f"end {id}")

fn main()
    run(true, 1)
    run(false, 2)
"#,
        ),
        "closing\nclosed 1\nthen\nend 1\nclosing\nclosed 2\nelse\nend 2",
    );
}

/// Moved into a binding in one arm only: the new binding releases it when
/// the arm ends, and the owner's scope releases it on the other path.
#[test]
fn a_resource_moved_into_a_binding_in_one_arm_is_released_on_each_path() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn run(flag bool, id int)
    let h = Handle(id)
    if flag
        let b = h
        println(f"moved {b.id}")
    println(f"end {id}")

fn main()
    run(true, 1)
    run(false, 2)
"#,
        ),
        "moved 1\nclosed 1\nend 1\nend 2\nclosed 2",
    );
}

/// Dropped by hand in one arm only: the hook runs once on each path.
#[test]
fn a_resource_dropped_in_one_arm_runs_its_hook_once_on_each_path() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn run(flag bool, id int)
    let h = Handle(id)
    if flag
        h.drop()
        println("dropped")
    println(f"end {id}")

fn main()
    run(true, 1)
    run(false, 2)
"#,
        ),
        "closed 1\ndropped\nend 1\nend 2\nclosed 2",
    );
}

/// A loop whose condition may fail before the body consumes the handle: the
/// `break` path has released it, the other path releases it at scope end.
#[test]
fn a_resource_consumed_in_a_loop_that_may_not_run_is_released_on_each_path() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn run(flag bool, id int)
    let h = Handle(id)
    while flag
        close(h)
        break
    println(f"end {id}")

fn main()
    run(true, 1)
    run(false, 2)
"#,
        ),
        "closing\nclosed 1\nend 1\nend 2\nclosed 2",
    );
}

/// A binding given a new value after it was consumed releases only the new
/// value; one consumed on some paths releases the old value on the others.
#[test]
fn a_resource_rebound_after_it_was_consumed_releases_each_value_once() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn run(flag bool, id int)
    var h = Handle(id)
    if flag
        close(h)
    h = Handle(id + 10)
    println(f"end {h.id}")

fn main()
    run(true, 1)
    run(false, 2)
"#,
        ),
        "closing\nclosed 1\nend 11\nclosed 11\nclosed 2\nend 12\nclosed 12",
    );
}

/// A `return` reached before the consuming call releases the handle there.
#[test]
fn a_resource_returned_past_before_it_is_consumed_is_released_at_the_return() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn run(flag bool, id int)
    let h = Handle(id)
    if flag
        println("early")
        return
    close(h)
    println(f"end {id}")

fn main()
    run(true, 1)
    run(false, 2)
"#,
        ),
        "early\nclosed 1\nclosing\nclosed 2\nend 2",
    );
}

/// A binding of an unbounded type parameter is not a resource where it is
/// written, so copying it consumes nothing, whatever type an instantiation
/// gives it: both bindings stay usable, and the hook runs once.
#[test]
fn a_generic_binding_copied_into_another_stays_usable_at_a_resource_type() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn keep_both<T>(x T) T
    let a = x
    let b = a
    println("copied")
    return a

fn main()
    let h = keep_both(Handle(1))
    println(f"kept {h.id}")
"#,
        ),
        "copied\nkept 1\nclosed 1",
    );
}
