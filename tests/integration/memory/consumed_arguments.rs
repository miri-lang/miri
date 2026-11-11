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

/// The call consumes the argument, so the caller's release of it lands at the
/// call: the hook runs once, before the caller goes on, and the aggregate's
/// other field is released where the aggregate is.
#[test]
fn a_parameter_built_into_a_local_aggregate_is_released_at_the_call() {
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
        "built\ndrop 9\ndrop 1\nafter 1",
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

/// The variant's argument is released once, at the call that consumed it.
#[test]
fn a_parameter_built_into_a_local_enum_variant_is_released_at_the_call() {
    assert_heap_guard_output(
        &format!(
            r#"{RES}
fn hold(a Res) int
    let e = Held.One(a)
    println("built")
    0

fn main()
    let h = Res(x: 1)
    let n = hold(h)
    println(f"after {{n}}")
"#
        ),
        "built\ndrop 1\nafter 0",
    );
}

/// `keeper` declares a function that only reads `h`, called as `call`; `h`
/// stays the caller's and is read after the call.
fn assert_lends(keeper: &str, call: &str) {
    assert_runs(&format!(
        "{RES}\n{keeper}\nfn main()\n    let h = Res(x: 1)\n    let k = Res(x: 2)\n    {call}\n    println(f\"{{h.x}}\")\n"
    ));
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
    );
}

#[test]
fn a_loop_binding_shadowing_a_parameter_hides_it() {
    assert_lends(
        &format!("{SHELF}fn f(r Res, xs [Res])\n    var s = Shelf()\n    for r in xs\n        s.slot = Some(r)"),
        "f(h, List([Res(x: 3)]))",
    );
}

#[test]
fn a_match_arm_local_shadowing_a_parameter_hides_it() {
    assert_lends(
        "fn f(r Res, n int) Res\n    match n\n        1: return Res(x: 5)\n        _:\n            let r = Res(x: 6)\n            return r\n    return Res(x: 0)",
        "let _v = f(h, 2)",
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
