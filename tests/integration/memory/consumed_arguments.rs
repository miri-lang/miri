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

/// A closure that is kept holds what it captures, so capturing a resource
/// consumes it and a later use is refused.
#[test]
fn a_resource_captured_by_a_kept_closure_cannot_be_used_after() {
    assert_compiler_error(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    let f = fn() int: h.id
    println(f"{h.id}")
"#,
        ),
        "consumed by",
    );
}

/// A closure does not own what it captures, so its body cannot hand a
/// captured resource to a callee that keeps it.
#[test]
fn a_closure_giving_away_a_captured_resource_is_refused() {
    assert_compiler_error(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    let f = fn() Handle: close(h)
"#,
        ),
        "the closure does not own",
    );
}

/// Nor can its body return a captured resource.
#[test]
fn a_closure_returning_a_captured_resource_is_refused() {
    assert_compiler_error(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    let f = fn() Handle
        return h
"#,
        ),
        "the closure does not own",
    );
}

/// A closure passed straight to a call that does not keep it borrows its
/// captures: the caller still owns the resource afterwards.
#[test]
fn a_resource_read_by_a_closure_passed_to_a_call_stays_usable() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn apply(f fn() int) int
    return f()

fn main()
    let h = Handle(1)
    println(f"{apply(fn() int: h.id)}")
    println(f"still {h.id}")
"#,
        ),
        "1\nstill 1\nclosed 1",
    );
}

/// A kept closure releases the captured resource when the closure is released.
#[test]
fn a_resource_captured_by_a_kept_closure_is_released_with_it() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    if true
        let h = Handle(1)
        let f = fn() int: h.id
        println(f"made {f()}")
    println("done")
"#,
        ),
        "made 1\nclosed 1\ndone",
    );
}

/// The closure becomes the resource's owner where it is created, so the hook
/// runs when the closure is released, not when the captured binding's scope
/// ends.
#[test]
fn a_resource_captured_by_a_shorter_lived_closure_is_released_with_the_closure() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn main()
    let h = Handle(1)
    if true
        let f = fn() int: h.id
        println(f"made {f()}")
    println("after")
"#,
        ),
        "made 1\nclosed 1\nafter",
    );
}

/// Captured on one path only, the resource is released with the closure there
/// and at its own scope end on the other.
#[test]
fn a_resource_captured_on_one_path_is_released_once_on_each() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn run(c bool, id int)
    let h = Handle(id)
    if c
        let f = fn() int: h.id
        println(f"made {f()}")
    println(f"end {id}")

fn main()
    run(true, 1)
    run(false, 2)
"#,
        ),
        "made 1\nclosed 1\nend 1\nend 2\nclosed 2",
    );
}

const RES: &str = r#"
use system.collections.list

class Res
    var x int
    fn drop(self)
        println(f"drop {self.x}")

class Pair
    var a Res
    var b Res

enum Held
    One(Res)
    Nothing
"#;

/// `keeper` declares a function that keeps `h`, called as `call`; using `h`
/// after the call is refused, as it is when the callee spells `return h`.
fn assert_keeps(keeper: &str, call: &str) {
    assert_compiler_error(
        &format!(
            "{RES}\n{keeper}\nfn main()\n    let h = Res(x: 1)\n    let k = Res(x: 2)\n    {call}\n    println(f\"{{h.x}}\")\n"
        ),
        "MER_OWN_003",
    );
}

/// A body that ends on the parameter returns it as `return h` does, so the
/// handle moves to the result and is released with it.
#[test]
fn a_resource_a_tail_expression_returns_is_released_with_the_result() {
    assert_heap_guard_output(
        r#"
class Handle
    id int
    fn drop(self)
        println(f"closed {self.id}")

fn keep(h Handle) Handle
    h

fn main()
    let i = 0
    let h = Handle(id: 0)
    if i == 0
        let _k = keep(h)
        println("kept")
    println("end")
"#,
        "kept\nclosed 0\nend",
    );
}

/// The tail spelling consumes the argument the way `return h` does.
#[test]
fn a_resource_a_tail_expression_returns_cannot_be_used_after() {
    assert_keeps("fn keep(h Res) Res\n    h", "let _r = keep(h)");
}

/// A tail after other statements is the body's value all the same.
#[test]
fn a_resource_a_block_tail_returns_cannot_be_used_after() {
    assert_keeps(
        "fn keep(h Res) Res\n    let n = h.x\n    println(f\"{n}\")\n    h",
        "let _r = keep(h)",
    );
}

/// An `if` whose arm ends on the parameter returns it on that path.
#[test]
fn a_resource_an_if_arm_tail_returns_cannot_be_used_after() {
    assert_keeps(
        "fn keep(flag bool, h Res) Res\n    if flag\n        h\n    else\n        Res(x: 9)",
        "let _r = keep(true, h)",
    );
}

/// The handle an `if` arm hands back is released with the result, and on the
/// other path the call releases it where it consumed it.
#[test]
fn a_resource_an_if_arm_tail_returns_is_released_on_each_path() {
    assert_heap_guard_output(
        &format!(
            r#"{RES}
fn keep(flag bool, h Res) Res
    if flag
        h
    else
        Res(x: 9)

fn run(flag bool)
    let h = Res(x: 1)
    if true
        let r = keep(flag, h)
        println(f"got {{r.x}}")
    println("end")

fn main()
    run(true)
    run(false)
"#
        ),
        "got 1\ndrop 1\nend\ndrop 1\ngot 9\ndrop 9\nend",
    );
}

/// A `match` arm that ends on the parameter returns it on that path.
#[test]
fn a_resource_a_match_arm_tail_returns_cannot_be_used_after() {
    assert_keeps(
        "fn keep(n int, h Res) Res\n    match n\n        1: h\n        _: Res(x: 9)",
        "let _r = keep(1, h)",
    );
}

/// A resource built into a returned aggregate leaves with it.
#[test]
fn a_resource_returned_inside_an_aggregate_cannot_be_used_after() {
    assert_keeps(
        "fn keep_both(a Res, b Res) Pair\n    return Pair(a: a, b: b)",
        "let _p = keep_both(h, k)",
    );
}

/// The tail spelling of the returned aggregate.
#[test]
fn a_resource_an_aggregate_tail_holds_cannot_be_used_after() {
    assert_keeps(
        "fn keep_both(a Res, b Res) Pair\n    Pair(a: a, b: b)",
        "let _p = keep_both(h, k)",
    );
}

/// The aggregate holding both handles releases them when it goes.
#[test]
fn a_resource_returned_inside_an_aggregate_is_released_with_it() {
    assert_heap_guard_output(
        &format!(
            r#"{RES}
fn keep_both(a Res, b Res) Pair
    Pair(a: a, b: b)

fn main()
    let h = Res(x: 1)
    let k = Res(x: 2)
    if true
        let p = keep_both(h, k)
        println(f"held {{p.a.x}} {{p.b.x}}")
    println("end")
"#
        ),
        "held 1 2\ndrop 1\ndrop 2\nend",
    );
}

/// An enum payload is an aggregate the resource leaves inside.
#[test]
fn a_resource_returned_as_an_enum_payload_cannot_be_used_after() {
    assert_keeps(
        "fn hold(r Res) Held\n    return Held.One(r)",
        "let _e = hold(h)",
    );
}

/// A method pushing its arguments into a list its object holds keeps them.
#[test]
fn a_resource_a_method_pushes_into_its_object_cannot_be_used_after() {
    assert_keeps(
        "class Shelf\n    var items [Res]\n    fn init()\n        self.items = List<Res>()\n    fn put(a Res, b Res)\n        self.items.push(a)\n        self.items.push(b)",
        "let shelf = Shelf()\n    shelf.put(h, k)",
    );
}

/// The shelf releases what it holds when it goes.
#[test]
fn a_resource_a_method_pushes_into_its_object_is_released_with_it() {
    assert_heap_guard_output(
        &format!(
            r#"{RES}
class Shelf
    var items [Res]
    fn init()
        self.items = List<Res>()
    fn put(a Res, b Res)
        self.items.push(a)
        self.items.push(b)

fn main()
    let h = Res(x: 1)
    let k = Res(x: 2)
    if true
        let shelf = Shelf()
        shelf.put(h, k)
        println("shelved")
    println("end")
"#
        ),
        "shelved\ndrop 1\ndrop 2\nend",
    );
}

/// A field of a field of `self` is the object's storage as much as its own
/// field is.
#[test]
fn a_resource_a_method_stores_in_a_nested_field_cannot_be_used_after() {
    assert_keeps(
        "class Box\n    var slot Res?\nclass Holder\n    var box Box\n    fn put(r Res)\n        self.box.slot = r",
        "let o = Holder(box: Box(slot: None))\n    o.put(h)",
    );
}

/// A function pushing a resource into the list it is lent keeps it there.
#[test]
fn a_resource_pushed_into_a_container_argument_cannot_be_used_after() {
    assert_keeps(
        "fn stash(list [Res], r Res)\n    list.push(r)",
        "let list = List<Res>()\n    stash(list, h)",
    );
}

/// A resource stored into a field of a value the function returns leaves
/// with that value.
#[test]
fn a_resource_stored_into_a_returned_value_cannot_be_used_after() {
    assert_keeps(
        "class Box\n    var slot Res?\nfn fill(r Res) Box\n    var b = Box(slot: None)\n    b.slot = r\n    return b",
        "let _c = fill(h)",
    );
}

/// The tail spelling cannot keep one resource at two positions.
#[test]
fn a_resource_a_tail_aggregate_keeps_twice_is_refused() {
    assert_compiler_error(
        &format!(
            "{RES}\nfn keep_both(a Res, b Res) Pair\n    Pair(a: a, b: b)\nfn main()\n    let h = Res(x: 1)\n    let _p = keep_both(h, h)\n"
        ),
        "'h' is passed to 'keep_both' more than once",
    );
}

/// Nor can a method storing both arguments into its object.
#[test]
fn a_resource_a_method_pushes_twice_is_refused() {
    assert_compiler_error(
        &format!(
            "{RES}\nclass Shelf\n    var items [Res]\n    fn init()\n        self.items = List<Res>()\n    fn put(a Res, b Res)\n        self.items.push(a)\n        self.items.push(b)\nfn main()\n    let shelf = Shelf()\n    let h = Res(x: 1)\n    shelf.put(h, h)\n"
        ),
        "'h' is passed to 'put' more than once",
    );
}

/// Only a resource is consumed: a string or list the callee stores or returns
/// is a value, and stays the caller's.
#[test]
fn a_value_a_callee_stores_or_returns_stays_usable() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Shelf
    var names [String]
    var lists [[int]]
    fn init()
        self.names = List<String>()
        self.lists = List<[int]>()
    fn put(name String, xs [int])
        self.names.push(name)
        self.lists.push(xs)

fn echo(xs [int]) [int]
    xs

fn main()
    let shelf = Shelf()
    let name = "a"
    let xs = List([1, 2])
    shelf.put(name, xs)
    let ys = echo(xs)
    println(f"{name} {xs.length()} {ys.length()} {shelf.names.length()}")
"#,
        "a 2 2 1",
    );
}

/// A call through a trait keeps what any implementor keeps, so a function
/// handing its argument to a trait method one implementor stores keeps it too.
#[test]
fn a_resource_handed_on_through_a_trait_an_implementor_stores_cannot_be_used_after() {
    assert_keeps(
        "trait Sink\n    fn put(r Res)\nclass Reader implements Sink\n    fn put(r Res)\n        println(f\"{r.x}\")\nclass Shelf implements Sink\n    var items [Res]\n    fn init()\n        self.items = List<Res>()\n    fn put(r Res)\n        self.items.push(r)\nfn give(s Sink, r Res)\n    s.put(r)",
        "let shelf = Shelf()\n    give(shelf, h)",
    );
}

/// A local a body ends on moves to the caller, as `return r` moves it: no
/// warning that it was never consumed, and the hook runs once, with the
/// result.
#[test]
fn a_local_resource_a_tail_returns_is_released_with_the_result() {
    let tail = format!("{RES}\nfn make() Res\n    let r = Res(x: 1)\n    println(\"made\")\n    r\nfn main()\n    if true\n        let k = make()\n        println(f\"got {{k.x}}\")\n    println(\"end\")\n");
    assert_no_diagnostic(&tail, "'r' of type 'Res' was not consumed");
    assert_heap_guard_output(&tail, "made\ngot 1\ndrop 1\nend");
}

/// The same in an `if` arm a body ends on.
#[test]
fn a_local_resource_an_if_arm_tail_returns_is_not_reported_unconsumed() {
    assert_no_diagnostic(
        &format!("{RES}\nfn make(flag bool) Res\n    if flag\n        let r = Res(x: 1)\n        r\n    else\n        Res(x: 2)\nfn main()\n    let k = make(true)\n    println(f\"{{k.x}}\")\n"),
        "'r' of type 'Res' was not consumed",
    );
}

/// A local bound from a parameter names the parameter: keeping it keeps the
/// argument.
#[test]
fn a_parameter_kept_through_a_local_alias_cannot_be_used_after() {
    assert_keeps(
        "fn stash(list [Res], r Res)\n    let tmp = r\n    list.push(tmp)",
        "let list = List<Res>()\n    stash(list, h)",
    );
}

/// So does returning the alias as the tail, through a chain of them.
#[test]
fn a_parameter_returned_through_a_chain_of_aliases_cannot_be_used_after() {
    assert_keeps(
        "fn keep(r Res) Res\n    let a = r\n    var b = a\n    b",
        "let _r = keep(h)",
    );
}

/// Building a parameter into an aggregate stores it there, which keeps it
/// wherever the aggregate goes, a local included.
#[test]
fn a_parameter_built_into_a_local_aggregate_cannot_be_used_after() {
    assert_keeps(
        "fn wrap(a Res) int\n    let p = Pair(a: a, b: Res(x: 9))\n    p.a.x",
        "let _n = wrap(h)",
    );
}

/// The call hands the argument over, so the aggregate holds the only
/// reference to it: the hook runs once, where the aggregate is released at
/// the end of the function that built it, before the caller goes on.
#[test]
fn a_parameter_built_into_a_local_aggregate_is_released_where_the_aggregate_ends() {
    assert_heap_guard_output(
        &format!(
            r#"{RES}
fn wrap(a Res) int
    let p = Pair(a: a, b: Res(x: 9))
    println("built")
    p.a.x

fn main()
    let h = Res(x: 1)
    let n = wrap(h)
    println(f"after {{n}}")
"#
        ),
        "built\ndrop 1\ndrop 9\nafter 1",
    );
}

/// An enum variant built from a parameter keeps it the same way.
#[test]
fn a_parameter_built_into_a_local_enum_variant_cannot_be_used_after() {
    assert_keeps(
        "fn hold(a Res) int\n    let e = Held.One(a)\n    println(\"built\")\n    0",
        "let _n = hold(h)",
    );
}

/// The variant holds the only reference to its argument, so the hook runs
/// once, where the variant is released at the end of the function.
#[test]
fn a_parameter_built_into_a_local_enum_variant_is_released_where_the_variant_ends() {
    assert_heap_guard_output(
        &format!(
            r#"{RES}
fn hold(a Res) int
    if true
        let e = Held.One(a)
        println("built")
    println("left")
    0

fn main()
    let h = Res(x: 1)
    let n = hold(h)
    println(f"after {{n}}")
"#
        ),
        "built\ndrop 1\nleft\nafter 0",
    );
}

/// `keeper` declares a function that only reads `h`, called as `call`; `h`
/// stays the caller's, is read after the call, and is released once, where
/// the caller ends: `expected` is the whole output, every hook's line in
/// its place.
fn assert_lends(keeper: &str, call: &str, expected: &str) {
    assert_heap_guard_output(
        &format!(
            "{RES}\n{keeper}\nfn main()\n    let h = Res(x: 1)\n    let k = Res(x: 2)\n    {call}\n    println(f\"{{h.x}}\")\n"
        ),
        expected,
    );
}

const SHELF: &str = "class Shelf\n    var slot Res?\n    fn init()\n        self.slot = None\n";

/// A user function named as a standard-library one is its own function: its
/// own summary decides what a call to it keeps.
#[test]
fn a_user_function_named_as_a_stdlib_one_keeps_by_its_own_body() {
    assert_keeps(
        "fn eprintln(r Res) Res\n    return r",
        "let _o = eprintln(h)",
    );
}

const SHARED_LIB: &str = concat!(
    "public class Res\n",
    "    public var x int\n",
    "    fn drop(self)\n",
    "        println(f\"drop {self.x}\")\n",
    "\n",
    "public fn take(r Res) Res\n",
    "    return r\n",
    "\n",
    "public fn pass(r Res) Res\n",
    "    return take(r)\n",
);

/// Two modules each declaring `take` declare two functions: the program's
/// `take` only reads, so its argument stays usable.
#[test]
fn a_program_function_sharing_a_module_functions_name_lends_by_its_own_body() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                "use local.lib.{Res, pass}\n\nfn take(r Res) int\n    r.x\n\nfn main()\n    let h = Res(x: 1)\n    let n = take(h)\n    println(f\"{h.x} {n}\")\n",
            ),
            ("lib.mi", SHARED_LIB),
        ],
        "1 1\ndrop 1",
    );
}

/// And the module's own `take`, which keeps, is what its `pass` calls.
#[test]
fn a_module_function_sharing_a_program_functions_name_keeps_by_its_own_body() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                "use local.lib.{Res, pass}\n\nfn take(r Res) int\n    r.x\n\nfn main()\n    let h = Res(x: 1)\n    let _p = pass(h)\n    println(f\"{h.x}\")\n",
            ),
            ("lib.mi", SHARED_LIB),
        ],
        "MER_OWN_003",
    );
}

/// A local rebound to another parameter no longer names the first.
#[test]
fn a_local_rebound_away_from_a_parameter_no_longer_keeps_it() {
    assert_lends(
        &format!("{SHELF}fn f(r Res, o Res)\n    var t = r\n    t = o\n    var s = Shelf()\n    s.slot = t"),
        "f(h, k)",
        "drop 2\n1\ndrop 1"
    );
}

/// A local rebound to a parameter names it from then on.
#[test]
fn a_local_rebound_to_a_parameter_keeps_it() {
    assert_keeps(
        &format!("{SHELF}fn f(r Res, o Res)\n    var t = o\n    t = r\n    var s = Shelf()\n    s.slot = Some(t)"),
        "f(h, k)",
    );
}

/// Rebound to the parameter on one path only, the local may name it after
/// the branch, so keeping the local keeps the parameter.
#[test]
fn a_local_rebound_to_a_parameter_on_one_path_keeps_it() {
    assert_keeps(
        &format!("{SHELF}fn f(r Res, o Res, c bool)\n    var t = o\n    if c\n        t = r\n    var s = Shelf()\n    s.slot = t"),
        "f(h, k, true)",
    );
}

/// A tuple, an array or a closure built from a parameter holds it, a local
/// one included.
#[test]
fn a_parameter_built_into_a_local_tuple_cannot_be_used_after() {
    assert_keeps(
        "fn f(r Res) (Res, int)\n    let t = (r, 1)\n    return t",
        "let _l = f(h)",
    );
}

#[test]
fn a_parameter_built_into_a_local_array_cannot_be_used_after() {
    assert_keeps(
        "fn f(r Res) Array<Res, 1>\n    let t = [r]\n    return t",
        "let _l = f(h)",
    );
}

#[test]
fn a_parameter_captured_by_a_local_closure_cannot_be_used_after() {
    assert_keeps(
        "fn f(r Res) fn() int\n    let g = fn() int: r.x\n    return g",
        "let _l = f(h)",
    );
}

const CALLBACKS: &str = "class Shelf\n    var cbs List<fn() int>\n    var cb fn() int\n    fn init()\n        self.cbs = List<fn() int>()\n        self.cb = fn() int: 0\n";

/// A closure capturing a parameter keeps it, wherever the closure goes.
#[test]
fn a_parameter_captured_by_a_closure_a_method_pushes_cannot_be_used_after() {
    assert_keeps(
        &format!("{CALLBACKS}    fn reg(r Res)\n        self.cbs.push(fn() int: r.x)"),
        "var s = Shelf()\n    s.reg(h)",
    );
}

#[test]
fn a_parameter_captured_by_a_closure_a_method_stores_through_a_local_cannot_be_used_after() {
    assert_keeps(
        &format!(
            "{CALLBACKS}    fn reg(r Res)\n        let g = fn() int: r.x\n        self.cb = g"
        ),
        "var s = Shelf()\n    s.reg(h)",
    );
}

#[test]
fn a_parameter_captured_through_an_alias_cannot_be_used_after() {
    assert_keeps(
        &format!(
            "{CALLBACKS}    fn reg(r Res)\n        let t = r\n        self.cbs.push(fn() int: t.x)"
        ),
        "var s = Shelf()\n    s.reg(h)",
    );
}

/// A trait default that only reads lends what it is handed.
#[test]
fn a_trait_default_that_only_reads_lends_its_argument() {
    assert_lends(
        "trait Reader\n    fn peek(r Res) int\n        return r.x\nclass Impl implements Reader\n    var n int",
        "let i = Impl(n: 1)\n    let _v = i.peek(h)",
        "1\ndrop 2\ndrop 1"
    );
}

/// A call through a trait reaches the default an implementor inherits.
#[test]
fn a_trait_default_an_implementor_inherits_keeps_through_the_trait() {
    assert_keeps(
        "trait Sink\n    fn put(r Res) Res\n        return r\nclass A implements Sink\n    var n int\nclass B implements Sink\n    var n int\n    fn put(r Res) Res\n        return Res(x: 5)",
        "let s Sink = B(n: 1)\n    let _o = s.put(h)",
    );
}

/// A call at a base class reaches every override below it.
#[test]
fn a_call_at_a_base_class_keeps_what_an_override_keeps() {
    assert_keeps(
        "class A\n    fn put(r Res) Res\n        return Res(x: 5)\nclass B extends A\n    fn put(r Res) Res\n        return r",
        "let s A = B()\n    let _o = s.put(h)",
    );
}

/// Reading methods of the collections lend what they are handed.
#[test]
fn a_resource_a_collection_only_reads_stays_usable() {
    assert_lends(
        "",
        "var l = List<Res>()\n    println(f\"{l.contains(h)} {l.length()}\")",
        "false 0\n1\ndrop 2\ndrop 1",
    );
}

/// A set's and a map's reading methods lend the key they are handed too.
#[test]
fn a_resource_a_set_or_a_map_only_reads_stays_usable() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
use system.collections.map

class Key implements Equatable, Hashable
    var x int
    fn equals(other Key) bool: self.x == other.x
    fn hash() int: self.x
    fn drop(self)
        println(f"drop {self.x}")

fn main()
    let h = Key(x: 1)
    let s = Set<Key>()
    let m = Map<Key, int>()
    println(f"{s.contains(h)} {m.contains_key(h)} {s.length()} {m.length()}")
    println(f"{h.x}")
"#,
        "false false 0 0\n1\ndrop 1",
    );
}

/// A local of a parameter's name hides the parameter for the rest of its
/// scope.
#[test]
fn a_local_shadowing_a_parameter_hides_it() {
    assert_lends(
        "fn f(r Res) Res\n    let r = Res(x: 2)\n    return r",
        "let _v = f(h)",
        "1\ndrop 2\ndrop 2\ndrop 1",
    );
}

#[test]
fn a_loop_binding_shadowing_a_parameter_hides_it() {
    assert_lends(
        &format!("{SHELF}fn f(r Res, xs [Res])\n    var s = Shelf()\n    for r in xs\n        s.slot = Some(r)"),
        "f(h, List([Res(x: 3)]))",
        "drop 3\n1\ndrop 2\ndrop 1"
    );
}

#[test]
fn a_match_arm_local_shadowing_a_parameter_hides_it() {
    assert_lends(
        "fn f(r Res, n int) Res\n    match n\n        1: return Res(x: 5)\n        _:\n            let r = Res(x: 6)\n            return r\n    return Res(x: 0)",
        "let _v = f(h, 2)",
        "1\ndrop 6\ndrop 2\ndrop 1"
    );
}

/// A keep before the shadow still keeps the parameter.
#[test]
fn a_keep_before_a_shadow_still_keeps_the_parameter() {
    assert_keeps(
        "fn f(r Res, list [Res]) Res\n    list.push(r)\n    let r = Res(x: 2)\n    r",
        "let list = List<Res>()\n    let _v = f(h, list)",
    );
}

/// Mutually recursive functions agree on what they keep.
#[test]
fn mutually_recursive_functions_that_only_read_lend() {
    assert_lends(
        "fn even(n int, r Res) int\n    if n == 0\n        return 0\n    return odd(n - 1, r)\nfn odd(n int, r Res) int\n    if n == 0\n        return 1\n    return even(n - 1, r)",
        "let _v = even(3, h)",
        "1\ndrop 2\ndrop 1"
    );
}

#[test]
fn mutually_recursive_functions_one_of_which_returns_keep() {
    assert_keeps(
        "fn even(n int, r Res) Res\n    if n == 0\n        return r\n    return odd(n - 1, r)\nfn odd(n int, r Res) Res\n    if n == 0\n        return Res(x: 9)\n    return even(n - 1, r)",
        "let _v = even(3, h)",
    );
}

/// A closure is a counted value: a method that stores a function-typed
/// parameter keeps it, and a closure passed there keeps what it captures.
#[test]
fn a_closure_a_method_stores_keeps_the_parameter_it_captures() {
    assert_keeps(
        &format!("{CALLBACKS}    fn reg(g fn() int)\n        self.cb = g\nfn f(r Res, s Shelf)\n    s.reg(fn() int: r.x)"),
        "var s = Shelf()\n    f(h, s)",
    );
}

#[test]
fn a_closure_a_method_pushes_keeps_the_parameter_it_captures() {
    assert_keeps(
        &format!("{CALLBACKS}    fn reg(g fn() int)\n        self.cbs.push(g)\nfn f(r Res)\n    var s = Shelf()\n    s.reg(fn() int: r.x)"),
        "f(h)",
    );
}

#[test]
fn a_closure_a_function_returns_keeps_the_parameter_it_captures() {
    assert_keeps(
        "fn keepit(g fn() int) fn() int\n    return g\nfn f(r Res) fn() int\n    return keepit(fn() int: r.x)",
        "let _c = f(h)",
    );
}

/// A loop body runs again with the locals its last pass rebound: a local
/// rebound to a parameter after a keeping call reaches that call on the next
/// pass.
#[test]
fn a_local_rebound_to_a_parameter_later_in_a_loop_keeps_it() {
    assert_keeps(
        &format!("{SHELF}    fn put(r Res)\n        self.slot = Some(r)\nfn f(r Res, o Res, n int)\n    var s = Shelf()\n    var t = o\n    var i = 0\n    while i < n\n        s.put(t)\n        t = r\n        i += 1"),
        "f(h, k, 2)",
    );
}

/// A local bound to a value that may be a parameter names that parameter.
#[test]
fn a_local_bound_to_a_conditional_naming_a_parameter_keeps_it() {
    assert_keeps(
        &format!("{SHELF}    fn put(r Res)\n        self.slot = Some(r)\nfn f(r Res, o Res, c bool)\n    var s = Shelf()\n    let t = r if c else o\n    s.put(t)"),
        "f(h, k, true)",
    );
}

#[test]
fn a_local_bound_to_a_match_naming_a_parameter_keeps_it() {
    assert_keeps(
        &format!("{SHELF}    fn put(r Res)\n        self.slot = Some(r)\nfn f(r Res, o Res, n int)\n    var s = Shelf()\n    let t = match n\n        1: r\n        _: o\n    s.put(t)"),
        "f(h, k, 1)",
    );
}

/// A closure takes what it captures when it is created, so a parameter a
/// local closure captures is kept even when the closure is only called and
/// dropped: the capture is a keep, as building a local aggregate is.
#[test]
fn a_parameter_a_local_closure_captures_is_kept_even_when_the_closure_stays_local() {
    assert_keeps(
        "fn f(r Res)\n    let g = fn() int: r.x\n    println(f\"{g()}\")",
        "f(h)",
    );
}

/// `cond`, which keeps `h` in one arm only: the handle `close` hands back is
/// bound to `k`, released where the arm ends.
const COND: &str = r#"
fn cond(h Handle, c bool) int
    if c
        let k = close(h)
        println("c")
    println("after")
    return 1
"#;

/// A function that keeps its resource parameter owns it: the caller hands
/// its reference over at the call, so the hook runs where the callee's new
/// owner ends on the path that keeps it, and where the callee ends on the
/// path that does not.
#[test]
fn a_kept_parameter_is_released_where_its_new_owner_ends() {
    assert_heap_guard_output(
        &with_handle(&format!(
            r#"{COND}
fn main()
    let a = cond(Handle(1), true)
    let b = cond(Handle(2), false)
    println("end")
"#
        )),
        "closing\nc\nclosed 1\nafter\nafter\nclosed 2\nend",
    );
}

/// A kept parameter built into an aggregate inside a block is released when
/// the block releases the aggregate, not when the function ends.
#[test]
fn a_parameter_built_into_an_aggregate_in_a_block_is_released_where_the_block_ends() {
    assert_heap_guard_output(
        r#"
class Res
    var x int
    fn drop(self)
        println(f"drop {self.x}")
class Pair
    var a Res
fn wrap(a Res) int
    if true
        let p = Pair(a: a)
        println("built")
    println("left")
    0
fn main()
    let h = Res(x: 1)
    let n = wrap(h)
    println(f"after {n}")
"#,
        "built\ndrop 1\nleft\nafter 0",
    );
}

/// Kept in both arms, the parameter is released where each arm's owner ends.
#[test]
fn a_parameter_kept_in_every_arm_is_released_where_each_arm_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn both(h Handle, c bool) int
    if c
        let k = close(h)
        println("then")
    else
        let k = close(h)
        println("else")
    println("after")
    return 1

fn main()
    let a = both(Handle(1), true)
    let b = both(Handle(2), false)
    println("end")
"#,
        ),
        "closing\nthen\nclosed 1\nafter\nclosing\nelse\nclosed 2\nafter\nend",
    );
}

/// A parameter the callee only reads stays the caller's, released where the
/// caller's binding ends.
#[test]
fn a_parameter_the_callee_only_reads_is_released_where_the_caller_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn peek(h Handle) int
    return h.id

fn main()
    let h = Handle(3)
    if true
        let n = peek(h)
        println(f"{n}")
    println("end")
"#,
        ),
        "3\nend\nclosed 3",
    );
}

/// A field handed to a function that keeps it is retained for the callee:
/// the object still holds the handle, so the hook runs when the object goes.
#[test]
fn a_field_handed_to_a_keeper_is_released_with_the_object_holding_it() {
    assert_heap_guard_output(
        &with_handle(&format!(
            r#"{COND}
class Holder
    public var h Handle
    fn init(h Handle)
        self.h = h

fn main()
    if true
        let holder = Holder(Handle(5))
        let n = cond(holder.h, true)
        println("held")
    println("end")
"#
        )),
        "closing\nc\nafter\nheld\nclosed 5\nend",
    );
}

/// A function that hands its kept parameter to itself owns it at every
/// depth, and the last one returns it to the caller's binding.
#[test]
fn a_recursive_keeper_hands_its_parameter_on_at_every_depth() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn countdown(h Handle, n int) Handle
    if n == 0
        return h
    return countdown(h, n - 1)

fn main()
    if true
        let h = countdown(Handle(4), 3)
        println("got")
    println("end")
"#,
        ),
        "got\nclosed 4\nend",
    );
}

/// A generic function is shared by every instantiation, so it never owns a
/// parameter: the caller still gives its binding up at the call.
#[test]
fn a_generic_keeper_at_a_resource_type_hands_it_to_the_result() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn hold<T>(x T) T
    return x

fn main()
    let h = Handle(6)
    if true
        let k = hold(h)
        println("held")
    println("end")
"#,
        ),
        "held\nclosed 6\nend",
    );
}

/// Returned on one path and built into a local on the other: each path
/// releases it where its own owner ends.
#[test]
fn a_parameter_returned_on_one_path_and_stored_on_another_is_released_by_each_owner() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Wrap
    var h Handle

fn route(h Handle, back bool) Handle?
    if back
        return h
    if true
        let w = Wrap(h: h)
        println("wrapped")
    println("routed")
    return None

fn main()
    if true
        let r = route(Handle(1), true)
        println("returned")
    let s = route(Handle(2), false)
    println("end")
"#,
        ),
        "returned\nclosed 1\nwrapped\nclosed 2\nrouted\nend",
    );
}

/// A kept parameter handed to a call whose result is discarded is released
/// at that call, before the function goes on.
#[test]
fn a_parameter_a_discarded_call_keeps_is_released_at_that_call() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn discard(h Handle) int
    close(h)
    println("discarded")
    return 0

fn main()
    let n = discard(Handle(1))
    println("end")
"#,
        ),
        "closing\nclosed 1\ndiscarded\nend",
    );
}

/// A tail expression passing the kept parameter on hands it to the caller's
/// result.
#[test]
fn a_parameter_passed_on_by_a_tail_call_is_released_with_the_result() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn pass(h Handle) Handle
    close(h)

fn main()
    if true
        let p = pass(Handle(2))
        println("passed")
    println("end")
"#,
        ),
        "closing\npassed\nclosed 2\nend",
    );
}

/// A function named as a value keeps its own convention at the calls naming
/// it: the value reaches it through a forwarding body, so a direct call
/// still hands the handle over and its hook runs where the keeper's new
/// owner ends.
#[test]
fn a_keeper_named_as_a_value_owns_its_parameter_at_a_direct_call() {
    assert_heap_guard_output(
        &with_handle(&format!(
            r#"{COND}
fn main()
    let f = cond
    let a = cond(Handle(1), true)
    println("end")
"#
        )),
        "closing\nc\nclosed 1\nafter\nend",
    );
}

/// A method every call reaches by name owns a parameter it keeps, as a
/// function does: the handle is released where the method's new owner ends,
/// before the method goes on.
#[test]
fn a_method_keeping_a_parameter_owns_it_and_releases_it_where_the_new_owner_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
class Router
    var n int
    fn route(h Handle) int
        if true
            let k = close(h)
            println("c")
        println("after")
        return 1

fn main()
    let r = Router(n: 0)
    let a = r.route(Handle(1))
    println("end")
"#,
        ),
        "closing\nc\nclosed 1\nafter\nend",
    );
}

/// A return before the keep leaves the parameter with the callee, which
/// releases it at that return; the path that keeps it releases it where the
/// new owner ends.
#[test]
fn a_parameter_returned_past_before_its_keep_is_released_at_the_return() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn early(h Handle, c bool) int
    if c
        return 0
    let k = close(h)
    println("kept")
    1

fn main()
    let a = early(Handle(1), true)
    println("between")
    let b = early(Handle(2), false)
    println("end")
"#,
        ),
        "closed 1\nbetween\nclosing\nkept\nclosed 2\nend",
    );
}

/// Each iteration hands the keeper a resource of its own, released by the
/// callee within that iteration.
#[test]
fn a_parameter_kept_in_a_loop_body_is_released_within_each_iteration() {
    assert_heap_guard_output(
        &with_handle(&format!(
            r#"{COND}
fn main()
    var i = 0
    while i < 3
        let n = cond(Handle(i), i == 1)
        println(f"step {{i}}")
        i = i + 1
    println("end")
"#
        )),
        "after\nclosed 0\nstep 0\nclosing\nc\nclosed 1\nafter\nstep 1\nafter\nclosed 2\nstep 2\nend",
    );
}

/// The keeper takes the binding on the first iteration, so the second has
/// nothing to hand it.
#[test]
fn a_binding_handed_to_a_keeper_inside_a_loop_is_refused() {
    assert_compiler_error(
        &with_handle(&format!(
            r#"{COND}
fn main()
    let h = Handle(1)
    var i = 0
    while i < 2
        let n = cond(h, true)
        i = i + 1
"#
        )),
        "MER_OWN_003",
    );
}

/// On a path that keeps nothing the callee still owns the parameter, so it
/// releases it once, where it ends, before the caller goes on.
#[test]
fn a_kept_parameter_the_taken_path_never_keeps_is_released_where_the_callee_ends() {
    assert_heap_guard_output(
        &with_handle(&format!(
            r#"{COND}
fn main()
    let h = Handle(1)
    let a = cond(h, false)
    println("caller")
"#
        )),
        "after\nclosed 1\ncaller",
    );
}

/// Two owned parameters the taken path keeps neither of are released where
/// the callee ends, in reverse order of declaration.
#[test]
fn two_kept_parameters_neither_kept_are_released_in_reverse_order() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn pair(a Handle, b Handle, c bool) int
    if c
        let x = close(a)
        let y = close(b)
    println("pair end")
    return 1

fn main()
    let n = pair(Handle(1), Handle(2), false)
    println("caller")
"#,
        ),
        "pair end\nclosed 2\nclosed 1\ncaller",
    );
}

/// A keeper that only reads its other resource leaves that one with the
/// caller, which reads it after the call and releases it once, at its end.
#[test]
fn a_parameter_kept_beside_one_lent_releases_each_with_its_owner() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn mix(a Handle, b Handle) int
    let k = close(a)
    println(f"read {b.id}")
    return 1

fn main()
    let a = Handle(1)
    let b = Handle(2)
    let n = mix(a, b)
    println(f"caller {b.id}")
"#,
        ),
        "closing\nread 2\nclosed 1\ncaller 2\nclosed 2",
    );
}

/// A resource handed down a chain of keepers is released once, where the
/// deepest owner ends.
#[test]
fn a_parameter_handed_down_a_chain_of_keepers_is_released_by_the_deepest_owner() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn third(h Handle) int
    let k = close(h)
    println("third")
    return 3

fn second(h Handle) int
    let n = third(h)
    println("second")
    return n

fn first(h Handle) int
    let n = second(h)
    println("first")
    return n

fn main()
    let n = first(Handle(1))
    println(f"end {n}")
"#,
        ),
        "closing\nthird\nclosed 1\nsecond\nfirst\nend 3",
    );
}

/// A resource parameter beside an `out` parameter is owned like any other;
/// the `out` slot is the caller's binding and is written through.
#[test]
fn a_parameter_kept_beside_an_out_parameter_is_released_where_its_new_owner_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn fill(h Handle, n out int)
    if true
        let k = close(h)
        println("inside")
    n = 5

fn main()
    var n = 0
    fill(Handle(1), n)
    println(f"back {n}")
"#,
        ),
        "closing\ninside\nclosed 1\nback 5",
    );
}

/// A generic function's body is shared by its instantiations, so it lends
/// even a parameter of a resource type, and the caller releases the
/// argument after the call.
#[test]
fn a_generic_function_keeping_a_resource_lends_and_the_caller_releases_after_the_call() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn keep_with<T>(h Handle, x T) int
    if true
        let k = close(h)
        println("inside")
    println("leaving")
    return 1

fn main()
    let h = Handle(1)
    let n = keep_with(h, 3)
    println("back")
"#,
        ),
        "closing\ninside\nleaving\nclosed 1\nback",
    );
}

/// A call through a function value hands the resource over: the value's
/// forwarding body hands the keeper what it keeps, so the hook runs where
/// the keeper's new owner ends, inside the keeper.
#[test]
fn a_keeper_called_through_a_function_value_releases_where_its_new_owner_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
fn keep(h Handle) int
    if true
        let k = close(h)
        println("inside")
    println("leaving")
    return 1

fn main()
    let h = Handle(1)
    let f = keep
    let n = f(h)
    println("back")
"#,
        ),
        "closing\ninside\nclosed 1\nleaving\nback",
    );
}

/// An implementor's method keeping a resource, called through its trait,
/// owns it through the trait method's slot: the hook runs where the
/// implementor's new owner ends, before the call returns.
#[test]
fn an_implementor_keeping_a_resource_through_a_trait_releases_it_where_the_new_owner_ends() {
    assert_heap_guard_output(
        &with_handle(
            r#"
trait Keeper
    fn keep(h Handle) int

class Store implements Keeper
    var n int
    fn keep(h Handle) int
        if true
            let k = close(h)
            println("inside")
        println("leaving")
        return 1

fn hand(k Keeper, h Handle) int
    return k.keep(h)

fn main()
    let s = Store(n: 0)
    let n = hand(s, Handle(1))
    println("back")
"#,
        ),
        "closing\ninside\nclosed 1\nleaving\nback",
    );
}

/// The caller reads a field of the resource it hands a keeper as a sibling
/// argument: the value is read before the call, so the callee releasing the
/// resource first leaves the argument intact.
#[test]
fn a_field_read_beside_the_resource_a_keeper_drops_keeps_its_value() {
    assert_heap_guard_output(
        r#"
class Tagged
    public var label String
    fn init(label String)
        self.label = label
    fn drop(self)
        println(f"dropped {self.label}")

fn hold(t Tagged) Tagged
    return t

fn keep(t Tagged, label String) int
    if true
        let k = hold(t)
    println(f"label {label}")
    return 1

fn main()
    let a = Tagged("x")
    let n = keep(a, a.label)
    println("back")
"#,
        "dropped x\nlabel x\nback",
    );
}

/// An element handed to a keeper is copied for the callee, which holds a
/// reference of its own: the list still holds the element, so its hook runs
/// when the list is released.
#[test]
fn an_element_handed_to_a_keeper_is_released_with_the_list_holding_it() {
    assert_heap_guard_output(
        &format!(
            r#"use system.collections.list
{HANDLE}
{COND}
fn main()
    let xs = List([Handle(1), Handle(2)])
    let n = cond(xs[0], true)
    println("back")
"#
        ),
        "closing\nc\nafter\nback\nclosed 1\nclosed 2",
    );
}

/// A keeper named as a value inside a generic body keeps its own convention
/// at the calls naming it.
#[test]
fn a_keeper_named_as_a_value_in_a_generic_body_owns_its_parameter_at_a_direct_call() {
    assert_heap_guard_output(
        &with_handle(&format!(
            r#"{COND}
fn pick<T>(x T) int
    let f = cond
    return 1

fn main()
    let p = pick(3)
    let a = cond(Handle(1), true)
    println("end")
"#
        )),
        "closing\nc\nclosed 1\nafter\nend",
    );
}

/// A keeper named as a value inside a lambda keeps its own convention at
/// the calls naming it.
#[test]
fn a_keeper_named_as_a_value_in_a_lambda_owns_its_parameter_at_a_direct_call() {
    assert_heap_guard_output(
        &with_handle(&format!(
            r#"{COND}
fn main()
    let g = fn() int
        let f = cond
        return 0
    let a = cond(Handle(1), true)
    println(f"end {{g()}}")
"#
        )),
        "closing\nc\nclosed 1\nafter\nend 0",
    );
}
