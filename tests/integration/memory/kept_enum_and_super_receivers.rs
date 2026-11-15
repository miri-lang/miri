// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A resource handed to a callee that keeps it is the callee's, wherever the
//! call is written: an enum's method keeping its receiver, a method handing
//! its receiver to a keeper through `super`, and a function handing a
//! parameter to a function value held in a local. The caller's binding is
//! consumed at the call, a later use is refused, and the drop hook runs
//! where the new owner ends. A callee that only reads what it is handed
//! lends it. Every run reads the exact order of the hooks' output under the
//! heap guard.

use super::super::utils::*;

const RES: &str = r#"
struct Shelf
    slot Res?

enum Res
    A(int)
    B

    fn drop(self)
        println("closed res")

    fn put_on(self) int
        if true
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
        println("leaving")
        return 0

    fn value(self) int
        match self
            Res.A(n): return n
            Res.B: return 0
"#;

fn with_res(program: &str) -> String {
    format!("{RES}\n{program}")
}

/// An enum method keeping its receiver owns it: the hook runs where the
/// shelf the method builds ends, before the method prints that it leaves.
#[test]
fn an_enum_method_keeping_its_receiver_releases_it_where_the_new_owner_ends() {
    assert_heap_guard_output(
        &with_res(
            r#"
fn main()
    let r = Res.A(1)
    r.put_on()
    println("after")
"#,
        ),
        "on shelf\nclosed res\nleaving\nafter",
    );
}

/// The receiver an enum method kept cannot be used again.
#[test]
fn an_enum_receiver_kept_by_its_method_cannot_be_used_after_the_call() {
    let program = with_res(
        r#"
fn main()
    let r = Res.A(1)
    r.put_on()
    println(f"{r.value()}")
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "'r' was consumed by 'Res.put_on'");
}

/// An enum method that only reads its receiver lends it: the binding stays
/// usable, and its hook runs once, where the caller's scope ends.
#[test]
fn an_enum_method_reading_its_receiver_lends_it() {
    assert_heap_guard_output(
        &with_res(
            r#"
fn main()
    let r = Res.A(4)
    println(f"{r.value()}")
    println(f"{r.value()}")
    println("after")
"#,
        ),
        "4\n4\nafter\nclosed res",
    );
}

/// An enum method keeping a resource parameter owns it, as a class's does.
#[test]
fn an_enum_method_keeping_a_parameter_releases_it_where_the_new_owner_ends() {
    assert_heap_guard_output(
        &with_res(
            r#"
enum Box
    Empty

    fn hold(r Res) int
        if true
            let _shelf = Shelf(slot: Some(r))
            println("held")
        println("leaving hold")
        return 0

fn main()
    let r = Res.B
    let b = Box.Empty
    b.hold(r)
    println("after")
"#,
        ),
        "held\nclosed res\nleaving hold\nafter",
    );
}

/// An enum hook handing its receiver to an enum method that keeps it lets
/// the receiver escape.
#[test]
fn an_enum_hook_handing_self_to_a_keeping_method_is_refused() {
    assert_compiler_error(
        r#"
struct Shelf
    slot Res?

enum Res
    A(int)
    B

    fn drop(self)
        self.put_on()

    fn put_on(self) int
        let _shelf = Shelf(slot: Some(self))
        return 0

fn main()
    let r = Res.A(1)
    println("made")
"#,
        "MER_OWN_007",
    );
}

const BASES: &str = r#"
struct Shelf
    slot Base?

class Base
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
    fn put_on() int
        if true
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
        println("leaving put_on")
        return 0
    fn peek() int
        return self.id

class Derived extends Base
    fn init(id int)
        super.init(id)
    fn relay() int
        let n = super.put_on()
        println("leaving relay")
        return n
    fn look() int
        return super.peek()
"#;

fn with_bases(program: &str) -> String {
    format!("{BASES}\n{program}")
}

/// A method handing its receiver through `super` to a base method that keeps
/// it keeps its receiver too: the hook runs inside the base method.
#[test]
fn a_receiver_handed_through_super_to_a_keeper_is_released_where_the_new_owner_ends() {
    assert_heap_guard_output(
        &with_bases(
            r#"
fn main()
    let d = Derived(1)
    d.relay()
    println("after")
"#,
        ),
        "on shelf\nclosed 1\nleaving put_on\nleaving relay\nafter",
    );
}

/// The receiver a method handed through `super` to a keeper cannot be used
/// after the call.
#[test]
fn a_receiver_handed_through_super_to_a_keeper_cannot_be_used_after_the_call() {
    let program = with_bases(
        r#"
fn main()
    let d = Derived(1)
    d.relay()
    println(f"{d.id}")
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "'d' was consumed by 'Derived.relay'");
}

/// A method lending its receiver through `super` to a base method that only
/// reads it lends it on: the binding stays usable.
#[test]
fn a_receiver_lent_through_super_stays_the_callers() {
    assert_heap_guard_output(
        &with_bases(
            r#"
fn main()
    let d = Derived(2)
    println(f"{d.look()}")
    println(f"{d.look()}")
    println("after")
"#,
        ),
        "2\n2\nafter\nclosed 2",
    );
}

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
        println("on shelf")
    println("leaving keep")
    return 0

fn look(h Handle) int
    return h.id
"#;

fn with_handle(program: &str) -> String {
    format!("{HANDLE}\n{program}")
}

/// A function handing its parameter to a keeper named through a local keeps
/// the parameter: its caller hands it over, and the hook runs in the keeper.
#[test]
fn a_parameter_handed_to_a_keeper_held_in_a_local_is_released_where_the_new_owner_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn pass(h Handle) int
    let f = keep
    let n = f(h)
    println("leaving pass")
    return n

fn main()
    let h = Handle(1)
    pass(h)
    println("after")
"#,
        ),
        "on shelf\nclosed 1\nleaving keep\nleaving pass\nafter",
    );
}

/// The parameter a function handed to a keeper held in a local is the
/// function's: its caller cannot use it after the call.
#[test]
fn a_parameter_handed_to_a_keeper_held_in_a_local_cannot_be_used_after_the_call() {
    let program = with_handle(
        r#"
fn pass(h Handle) int
    let f = keep
    return f(h)

fn main()
    let h = Handle(1)
    pass(h)
    println(f"{h.id}")
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "'h' was consumed by 'pass'");
}

/// A lambda literal held in a local that keeps its argument keeps the
/// parameter it is handed.
#[test]
fn a_parameter_handed_to_a_keeping_lambda_held_in_a_local_cannot_be_used_after_the_call() {
    let program = with_handle(
        r#"
fn pass(h Handle) int
    let f = fn(x Handle) int
        return keep(x)
    return f(h)

fn main()
    let h = Handle(1)
    pass(h)
    println(f"{h.id}")
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
}

/// A function value a function cannot resolve — one it was handed, here
/// through a local — keeps what it is handed.
#[test]
fn a_parameter_handed_to_an_unresolved_function_value_in_a_local_is_kept() {
    let program = with_handle(
        r#"
fn pass(h Handle, g fn(Handle) int) int
    let f = g
    return f(h)

fn main()
    let h = Handle(1)
    pass(h, look)
    println(f"{h.id}")
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
}

/// A var holding a function value that is reassigned cannot be resolved to
/// one body, so it keeps what it is handed.
#[test]
fn a_parameter_handed_to_a_reassigned_function_value_is_kept() {
    let program = with_handle(
        r#"
fn pass(h Handle, c bool) int
    var f = look
    if c
        f = keep
    return f(h)

fn main()
    let h = Handle(1)
    pass(h, true)
    println(f"{h.id}")
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
}

/// A function handing its parameter to a reader held in a local, named or a
/// lambda literal, lends it: the binding stays usable and its hook runs once,
/// where the caller's scope ends.
#[test]
fn a_parameter_handed_to_a_reader_held_in_a_local_stays_the_callers() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn pass(h Handle) int
    let f = look
    return f(h)

fn pass_lambda(h Handle) int
    let f = fn(x Handle) int
        return x.id
    return f(h)

fn main()
    let h = Handle(3)
    println(f"{pass(h)}")
    println(f"{pass_lambda(h)}")
    println("after")
"#,
        ),
        "3\n3\nafter\nclosed 3",
    );
}

const CHAIN: &str = r#"
struct Shelf
    slot A?

class A
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
    fn put_on() int
        if true
            let _shelf = Shelf(slot: Some(self))
            println("on shelf")
        println("leaving put_on")
        return 0
    fn peek() int
        return self.id

class B extends A
    fn init(id int)
        super.init(id)

class C extends B
    fn init(id int)
        super.init(id)
    fn relay() int
        let n = super.put_on()
        println("leaving relay")
        return n
    fn relay_branch(c bool) int
        if c
            super.put_on()
        println("x")
        return 0
"#;

fn with_chain(methods: &str, program: &str) -> String {
    format!("{CHAIN}{methods}\n{program}")
}

/// `super` reaches a keeper two classes up: the grandparent's body keeps
/// the receiver, and the hook runs inside it.
#[test]
fn a_receiver_handed_through_super_to_a_grandparents_keeper_is_released_there() {
    assert_heap_guard_output(
        &with_chain(
            "",
            r#"
fn main()
    let c = C(1)
    c.relay()
    println("after")
"#,
        ),
        "on shelf\nclosed 1\nleaving put_on\nleaving relay\nafter",
    );
}

/// A receiver handed through `super` on one branch is released by the
/// keeper on that path, and by the method's scope end on the other.
#[test]
fn a_receiver_handed_through_super_on_one_branch_is_released_once_on_each_path() {
    assert_heap_guard_output(
        &with_chain(
            "",
            r#"
fn main()
    let c = C(1)
    c.relay_branch(true)
    println("after true")
    let d = C(2)
    d.relay_branch(false)
    println("after false")
"#,
        ),
        "on shelf\nclosed 1\nleaving put_on\nx\nafter true\nx\nclosed 2\nafter false",
    );
}

/// The refusal of a use of `super` after a call through `super` kept it.
fn assert_super_reuse_refused(method: &str) {
    let program = with_chain(
        method,
        r#"
fn main()
    let c = C(1)
    println("made")
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "'self' was consumed by 'A.put_on'");
}

#[test]
fn a_second_call_through_super_after_a_keep_is_refused() {
    assert_super_reuse_refused(
        r#"    fn twice() int
        super.put_on()
        return super.put_on()
"#,
    );
}

#[test]
fn a_call_through_super_keeping_in_a_loop_is_refused() {
    assert_super_reuse_refused(
        r#"    fn looped() int
        for i in 0..2
            super.put_on()
        return 0
"#,
    );
}

#[test]
fn a_read_through_super_after_a_keep_is_refused() {
    assert_super_reuse_refused(
        r#"    fn then_peek() int
        super.put_on()
        return super.peek()
"#,
    );
}

/// A function value a pattern binds is one the function cannot resolve to
/// one body, so a call through it keeps what it is handed.
#[test]
fn a_parameter_handed_to_a_function_value_a_pattern_binds_is_kept() {
    let program = with_handle(
        r#"
fn pass(h Handle, g (fn(Handle) int)?) int
    match g
        Some(f): return f(h)
        None: return 0

fn main()
    let h = Handle(1)
    pass(h, Some(look))
    println(f"{h.id}")
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, "'h' was consumed by 'pass'");
}

/// A function value rebound later in a loop body may be the rebound value
/// at the call on the next pass, so the call keeps what it is handed.
#[test]
fn a_parameter_handed_to_a_function_value_rebound_in_a_loop_is_kept() {
    let program = with_handle(
        r#"
fn pass(h Handle) int
    var f = look
    var n = 0
    while n < 3
        if n == 2
            return f(h)
        f = keep
        n = n + 1
    return 0

fn main()
    let h = Handle(1)
    pass(h)
    println(f"{h.id}")
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
}

/// A function value rebound on the `else` branch only may be either value
/// after the `if`, so the call keeps what it is handed.
#[test]
fn a_parameter_handed_to_a_function_value_rebound_in_an_else_branch_is_kept() {
    let program = with_handle(
        r#"
fn pass(h Handle, c bool) int
    var f = look
    if c
        println("kept look")
    else
        f = keep
    return f(h)

fn main()
    let h = Handle(1)
    pass(h, true)
    println(f"{h.id}")
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
}

/// Everyday closures keep working: a lambda handed to `map`, and a lambda
/// stored in a local called with values that are not resources.
#[test]
fn everyday_closures_over_plain_values_run() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn total(xs [int]) int
    let add = fn(a int, b int) int: a + b
    var sum = 0
    for x in xs.map(fn(x int) int: x * 10)
        sum = add(sum, x)
    return sum

fn main()
    let xs = List([1, 2, 3])
    println(f"{total(xs)}")
    println(f"{xs.length()}")
"#,
        "60\n3",
    );
}
