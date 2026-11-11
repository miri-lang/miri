// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A call through a function value or a closure owns every resource it is
//! handed: the value's calling convention takes over each argument position
//! whose declared type runs a drop hook of its own. A lambda binds what it is
//! handed as a local of its own, so the hook runs where that local's owner
//! ends; a function named as a value is reached through a forwarding body
//! that hands the function what it keeps and releases the rest once the
//! function returns. Every test reads the exact order of the hook's output
//! under the heap guard.

use super::super::utils::*;

const HANDLE: &str = r#"
struct Shelf
    slot Handle?

class Handle
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

fn keep(h Handle) int
    if true
        let _shelf = Shelf(slot: Some(h))
        println("kept")
    println("leaving keep")
    return 0

fn look(h Handle) int
    println(f"look {h.id}")
    return h.id
"#;

fn with_handle(program: &str) -> String {
    format!("{HANDLE}\n{program}")
}

/// A lambda keeping its argument releases it where the shelf it builds
/// ends, inside the lambda.
#[test]
fn a_lambda_keeping_its_argument_releases_it_where_the_new_owner_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let f = fn(h Handle) int
        if true
            let _shelf = Shelf(slot: Some(h))
            println("kept")
        println("leaving lambda")
        return 0
    f(Handle(1))
    println("after closure")
"#,
        ),
        "kept\nclosed 1\nleaving lambda\nafter closure",
    );
}

/// A lambda that only reads its argument still owns it, and releases it
/// where the lambda ends, before the caller goes on.
#[test]
fn a_lambda_reading_its_argument_releases_it_where_the_lambda_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let r = fn(h Handle) int
        println(f"read {h.id}")
        println("leaving reader")
        return 0
    let h2 = Handle(2)
    r(h2)
    println("after reader")
"#,
        ),
        "read 2\nleaving reader\nclosed 2\nafter reader",
    );
}

/// A keeper named as a value and also called by name keeps its argument on
/// both paths, and in every round of a loop handing the value a fresh
/// resource: the hook runs inside the keeper each time.
#[test]
fn a_keeper_named_as_a_value_keeps_its_argument_through_the_value_and_directly() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let k = keep
    k(Handle(4))
    println("after keeper value")
    keep(Handle(5))
    println("after keeper direct")
    for i in 0..2
        k(Handle(i))
    println("done")
"#,
        ),
        "kept\nclosed 4\nleaving keep\nafter keeper value\n\
         kept\nclosed 5\nleaving keep\nafter keeper direct\n\
         kept\nclosed 0\nleaving keep\nkept\nclosed 1\nleaving keep\ndone",
    );
}

/// A function that only reads its argument, called through a value, is
/// handed the argument by the value and lends it to the function, then
/// releases it once the function returns, before the caller goes on.
#[test]
fn a_reader_named_as_a_value_releases_its_argument_once_the_reader_returns() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let g = look
    let h2 = Handle(2)
    let n = g(h2)
    println(f"after value {n}")
    let h3 = Handle(3)
    look(h3)
    println("after direct")
"#,
        ),
        "look 2\nclosed 2\nafter value 2\nlook 3\nafter direct\nclosed 3",
    );
}

/// A field handed to a keeper through a function value is copied for the
/// call, funded with a reference of its own: the keeper's shelf releases
/// that reference without running the hook, since the holder still holds
/// the value, and the hook runs once, where the holder ends.
#[test]
fn a_field_handed_to_a_keeper_value_runs_its_hook_once_where_the_holder_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
struct Holder
    h Handle

fn main()
    let k = keep
    let holder = Holder(h: Handle(8))
    k(holder.h)
    println(f"field {holder.h.id}")
"#,
        ),
        "kept\nleaving keep\nfield 8\nclosed 8",
    );
}

/// A function value in a generic body, instantiated at the resource, hands
/// its lent parameter on with a reference of its own: the caller of the
/// generic function still holds the argument and releases it after the call.
#[test]
fn a_function_value_in_an_instantiated_generic_body_funds_its_lent_argument() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn apply<T>(f fn(T) int, x T) int
    return f(x)

fn main()
    let n = apply(keep, Handle(6))
    println(f"after apply {n}")
"#,
        ),
        "kept\nleaving keep\nclosed 6\nafter apply 0",
    );
}

/// A function value with a plain parameter beside a resource one owns only
/// the resource.
#[test]
fn a_function_value_owns_only_its_resource_parameter() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn tag(n int, h Handle) int
    if true
        let _shelf = Shelf(slot: Some(h))
        println(f"tagged {n}")
    println("leaving tag")
    return n

fn main()
    let t = tag
    let n = t(7, Handle(7))
    println(f"after tag {n}")
"#,
        ),
        "tagged 7\nclosed 7\nleaving tag\nafter tag 7",
    );
}

/// A resource handed to a function value is the value's: reading it after
/// the call is refused.
#[test]
fn a_resource_read_after_a_function_value_took_it_is_refused() {
    assert_compiler_error(
        &with_handle(
            r#"
fn main()
    let g = look
    let h = Handle(1)
    g(h)
    println(f"{h.id}")
"#,
        ),
        "MER_OWN_003",
    );
}

/// A nested function calls itself through its own value: each level owns
/// the handle it was given and releases it where that level ends.
#[test]
fn a_nested_function_recursing_through_its_value_releases_each_level_where_it_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    fn countdown(h Handle, n int) int
        println(f"enter {h.id}")
        if n > 0
            countdown(Handle(h.id + 1), n - 1)
        println(f"leave {h.id}")
        return 0
    countdown(Handle(1), 2)
    println("after recursion")
"#,
        ),
        "enter 1\nenter 2\nenter 3\nleave 3\nclosed 3\nleave 2\nclosed 2\n\
         leave 1\nclosed 1\nafter recursion",
    );
}

/// A function recursing by name, also named as a value: the value owns the
/// outermost handle and releases it once the function returns.
#[test]
fn a_recursive_function_named_as_a_value_releases_each_handle_once() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn descend(h Handle, n int) int
    println(f"enter {h.id}")
    if n > 0
        descend(Handle(h.id + 1), n - 1)
    println(f"leave {h.id}")
    return 0

fn main()
    let d = descend
    d(Handle(1), 1)
    println("after recursion")
"#,
        ),
        "enter 1\nenter 2\nleave 2\nclosed 2\nleave 1\nclosed 1\nafter recursion",
    );
}

/// A lambda owning two resources releases both where it ends, in reverse
/// order of declaration.
#[test]
fn a_lambda_releases_its_resource_parameters_in_reverse_declaration_order() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let f = fn(a Handle, b Handle) int
        println(f"pair {a.id} {b.id}")
        return 0
    f(Handle(1), Handle(2))
    println("after pair")
"#,
        ),
        "pair 1 2\nclosed 2\nclosed 1\nafter pair",
    );
}

/// A lambda keeping only its first resource releases it where the shelf
/// ends, and the one it only reads where the lambda ends.
#[test]
fn a_lambda_keeping_one_of_two_resources_releases_each_where_its_owner_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let f = fn(a Handle, b Handle) int
        println(f"keeping {a.id}")
        if true
            let _shelf = Shelf(slot: Some(a))
        println(f"leaving with {b.id}")
        return 0
    f(Handle(1), Handle(2))
    println("after mix")
"#,
        ),
        "keeping 1\nclosed 1\nleaving with 2\nclosed 2\nafter mix",
    );
}

/// A function keeping only its first resource, reached through a value: the
/// forwarding body hands it exactly the resource it keeps, and releases the
/// one it only reads once the function returns.
#[test]
fn a_keeper_value_of_one_of_two_resources_is_handed_exactly_what_it_keeps() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn pick(a Handle, b Handle) int
    println(f"keeping {a.id}")
    if true
        let _shelf = Shelf(slot: Some(a))
    println(f"leaving with {b.id}")
    return 0

fn main()
    let p = pick
    p(Handle(1), Handle(2))
    println("after mix")
"#,
        ),
        "keeping 1\nclosed 1\nleaving with 2\nclosed 2\nafter mix",
    );
}

/// A closure stored in a field and called later owns the resource it is
/// handed, as any call through a value does.
#[test]
fn a_closure_called_through_a_field_releases_its_argument_where_it_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Box
    var cb fn(Handle) int
    fn init(cb fn(Handle) int)
        self.cb = cb

fn main()
    let b = Box(fn(h Handle) int: look(h))
    b.cb(Handle(1))
    println("after field call")
"#,
        ),
        "look 1\nclosed 1\nafter field call",
    );
}

/// A closure a function returns, called at once, owns the resource it is
/// handed.
#[test]
fn a_returned_closure_releases_its_argument_where_it_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn mk() fn(Handle) int
    return fn(h Handle) int: look(h)

fn main()
    mk()(Handle(1))
    println("after returned call")
"#,
        ),
        "look 1\nclosed 1\nafter returned call",
    );
}

/// A lambda keeping its resource on one branch only releases it inside the
/// branch on that path and at its end on the other, each hook once.
#[test]
fn a_lambda_keeping_on_one_branch_releases_once_on_each_path() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let f = fn(h Handle, c bool) int
        if c
            let _shelf = Shelf(slot: Some(h))
            println("kept")
        println("leaving")
        return 0
    f(Handle(1), true)
    f(Handle(2), false)
    println("after branches")
"#,
        ),
        "kept\nclosed 1\nleaving\nleaving\nclosed 2\nafter branches",
    );
}

/// A lambda capturing a resource and handed another releases the one it is
/// handed where it ends, and the captured one with the closure.
#[test]
fn a_lambda_capturing_a_resource_and_taking_one_releases_each_with_its_owner() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let c = Handle(9)
    let f = fn(h Handle) int
        println(f"cap {c.id} arg {h.id}")
        return 0
    f(Handle(1))
    println("after capture")
"#,
        ),
        "cap 9 arg 1\nclosed 1\nafter capture\nclosed 9",
    );
}

/// A resource handed to a lambda is the lambda's: reading it after the call
/// is refused.
#[test]
fn a_resource_read_after_a_lambda_took_it_is_refused() {
    assert_compiler_error(
        &with_handle(
            r#"
fn main()
    let f = fn(h Handle) int: h.id
    let h = Handle(1)
    f(h)
    println(f"{h.id}")
"#,
        ),
        "MER_OWN_003",
    );
}

/// Handing one resource to a function value in every round of a loop is
/// refused: the second round would hand over what the first took.
#[test]
fn a_resource_handed_to_a_function_value_in_a_loop_is_refused() {
    assert_compiler_error(
        &with_handle(
            r#"
fn main()
    let k = keep
    let h = Handle(1)
    for i in 0..2
        k(h)
"#,
        ),
        "MER_OWN_003",
    );
}

/// A closure taking only plain values lends them: the caller's values stay
/// readable after the call.
#[test]
fn a_closure_taking_plain_values_leaves_them_readable() {
    assert_heap_guard_output(
        r#"
fn main()
    let f = fn(n int, s String) int
        println(f"in {n} {s}")
        return n
    let n = 3
    let s = "hi"
    f(n, s)
    println(f"out {n} {s}")
"#,
        "in 3 hi\nout 3 hi",
    );
}

/// A lambda inside a generic body, instantiated at the resource, hands the
/// value it owns on to a function value: the generic body lends its values,
/// so the hand-over is funded and each hook runs once.
#[test]
fn a_lambda_in_an_instantiated_generic_body_hands_its_value_on_once() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn twice<T>(x T, g fn(T) int) int
    let f = fn(y T) int: g(y)
    return f(x)

fn main()
    let n = twice(Handle(1), look)
    println(f"after twice {n}")
"#,
        ),
        "look 1\nclosed 1\nafter twice 1",
    );
}
