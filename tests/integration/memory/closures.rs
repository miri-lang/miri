// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for memory correctness around lambda captures of managed values.
// A lambda that captures a managed value must IncRef it on creation and
// DecRef it when the lambda itself is dropped.
//
// Every test runs under the heap guard, which tracks a closure's block like a
// class instance's and reports one left behind at exit or read after it was
// freed: a clean run proves no leak and no double free, not only the output.
//
// Covered cases:
//   - Primitive captures (int, float, bool) — copy semantics, no RC needed
//   - String captures — pointer capture with correct RC
//   - Class captures (primitive fields and List fields)
//   - List captures — closure env DecRefs captured List on drop

use super::super::utils::*;

// ────────────────────────────────────────────────────────────────────────────
//  Working captures
// ────────────────────────────────────────────────────────────────────────────

/// Lambda captures a String; RC must be balanced across calls and drop.
/// This extends the basic capture test with an explicit memory-leak assertion.
#[test]
fn test_lambda_captures_string_no_leak() {
    assert_heap_guard_output(
        r#"

fn main()
    let greeting = "hello"
    let greet = fn() String: greeting
    println(greet())
    println(greet())
"#,
        "hello\nhello",
    );
}

/// Lambda captures a class instance whose fields are all primitives.
/// The class pointer must be IncRef'd on lambda creation and DecRef'd on drop.
#[test]
fn test_lambda_captures_class_no_leak() {
    assert_heap_guard_output(
        r#"

class Counter
    var value int

fn main()
    let c = Counter(value: 42)
    let get = fn() int: c.value
    println(f"{get()}")
"#,
        "42",
    );
}

/// Lambda captures two Strings; both must be freed when the lambda drops.
#[test]
fn test_lambda_captures_two_strings_no_leak() {
    assert_heap_guard_output(
        r#"

fn main()
    let first = "hello"
    let second = "world"
    let both = fn() String: first
    println(both())
    println(second)
"#,
        "hello\nworld",
    );
}

/// Lambda called multiple times with a captured String; no double-free.
#[test]
fn test_lambda_captures_string_called_multiple_times_no_leak() {
    assert_heap_guard_output(
        r#"

fn main()
    let label = "item"
    let print_label = fn() String: label
    println(print_label())
    println(print_label())
    println(print_label())
"#,
        "item\nitem\nitem",
    );
}

// ────────────────────────────────────────────────────────────────────────────
// ────────────────────────────────────────────────────────────────────────────

/// Lambda capturing a List; the List must not be freed while the lambda is live,
/// and must be freed exactly once after both the lambda and outer binding drop.
#[test]
fn test_lambda_captures_list_no_leak() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let items = List([1, 2, 3])
    let size = fn() int: items.length()
    println(f"{size()}")
"#,
        "3",
    );
}

/// Lambda called multiple times; the captured List must not be double-freed.
#[test]
fn test_lambda_called_multiple_times_no_leak() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let data = List([10, 20, 30])
    let len = fn() int: data.length()
    println(f"{len()}")
    println(f"{len()}")
    println(f"{len()}")
"#,
        "3\n3\n3",
    );
}

/// Lambda captures two managed Lists; both must be freed when the lambda drops.
#[test]
fn test_lambda_captures_two_lists_no_leak() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let a = List([1, 2])
    let b = List([3, 4, 5])
    let combined = fn() int: a.length() + b.length()
    println(f"{combined()}")
"#,
        "5",
    );
}

/// Lambda created fresh inside a loop, capturing the same outer List each time.
#[test]
fn test_lambda_created_in_loop_no_accumulation() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let base = List([1, 2, 3])
    var total = 0
    for i in 0..4
        let f = fn() int: base.length()
        total = total + f()
    println(f"{total}")
    println(f"{base.length()}")
"#,
        "12\n3",
    );
}

/// Lambda passed as argument to a higher-order function, captures a List.
#[test]
fn test_lambda_passed_as_arg_captures_list_no_leak() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn apply(f fn() int) int
    f()

fn main()
    let items = List([1, 2, 3, 4])
    let result = apply(fn() int: items.length())
    println(f"{result}")
"#,
        "4",
    );
}

/// Lambda captures a class with a managed List field.
#[test]
fn test_lambda_captures_class_with_list_field_no_leak() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Bag
    var items [int]

fn main()
    let bag = Bag(items: List([5, 10, 15]))
    let bag_len = fn() int: bag.items.length()
    println(f"{bag_len()}")
    println(f"{bag.items.length()}")
"#,
        "3\n3",
    );
}

/// Mutable closure var reassigned from a capturing closure to a non-capturing one.
/// Releasing the closure the variable held must release that closure's
/// captures, and nothing on the new closure's payload (which has none).
#[test]
fn test_closure_var_reassigned_capturing_to_non_capturing() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let data = List([1, 2, 3])
    var f = fn() int: data.length()
    f = fn() int: 99
    println(f"{f()}")
"#,
        "99",
    );
}

/// Mutable closure var reassigned from a non-capturing closure to a capturing one.
/// Ensures the new captures are correctly tracked and freed on drop.
#[test]
fn test_closure_var_reassigned_non_capturing_to_capturing() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let data = List([10, 20, 30])
    var f = fn() int: 42
    f = fn() int: data.length()
    println(f"{f()}")
"#,
        "3",
    );
}

// ────────────────────────────────────────────────────────────────────────────
// ────────────────────────────────────────────────────────────────────────────

/// A closure that is created and dropped leaves no block behind: its
/// environment is reported to the heap guard as a class block on allocation
/// and on release, so a correct program exits without a leak report.
#[test]
fn a_dropped_closure_leaves_no_block() {
    assert_heap_guard_output(
        r#"

fn main()
    let f = fn() int: 42
    println(f"{f()}")
"#,
        "42",
    );
}

/// A closure environment that is never released is reported by the heap
/// guard as a leaked class block, the same report any inline block gets.
///
/// The leak is a cycle: the closure captures the holder, and the holder keeps
/// the closure, so neither count reaches zero and exactly those two blocks
/// are reported. `a_closure_kept_by_an_object_it_does_not_capture_is_released`
/// is the same program without the cycle, and exits clean.
#[test]
fn test_leaked_closure_environment_is_reported_as_a_class_block() {
    assert_heap_guard_detects(
        r#"
class Holder
    public var value int
    public var callback (fn() int)?
    public fn init(value int)
        self.value = value
        self.callback = None

fn main()
    var holder = Holder(7)
    let read = fn() int: holder.value
    holder.callback = Some(read)
    println("built")
"#,
        &["leaked 2 allocations", "(class) x2"],
    );
}

/// An object keeping a closure that captures a different object is released
/// with the closure and its capture: without a cycle nothing is left behind.
#[test]
fn a_closure_kept_by_an_object_it_does_not_capture_is_released() {
    assert_heap_guard_output(
        r#"
class Holder
    public var value int
    public var callback (fn() int)?
    public fn init(value int)
        self.value = value
        self.callback = None

fn main()
    var holder = Holder(7)
    let other = Holder(8)
    let read = fn() int: other.value
    holder.callback = Some(read)
    println("built")
"#,
        "built",
    );
}

/// `system.testing.simulate_closure_leak` leaks one class block, which the
/// heap guard reports at exit.
#[test]
fn test_simulated_closure_leak_is_reported_as_a_class_block() {
    assert_heap_guard_detects(
        r#"
use system.testing

fn main()
    simulate_closure_leak()
"#,
        &["leaked 1 allocations", "(class)"],
    );
}

/// Two simulated leaks are reported together.
#[test]
fn test_simulated_closure_leak_reports_count() {
    assert_heap_guard_detects(
        r#"
use system.testing

fn main()
    simulate_closure_leak()
    simulate_closure_leak()
"#,
        &["leaked 2 allocations", "(class)"],
    );
}

/// Same with a capturing closure to verify that both the closure struct and its
/// captured managed value remain balanced across the full alloc → use → drop cycle.
#[test]
fn a_dropped_capturing_closure_leaves_no_block() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let items = List([1, 2, 3])
    let f = fn() int: items.length()
    println(f"{f()}")
"#,
        "3",
    );
}

// ────────────────────────────────────────────────────────────────────────────
// ────────────────────────────────────────────────────────────────────────────

/// Closure created inside a function and returned to the caller.
/// The captured List must survive the creator's scope exit (IncRef at capture)
/// and be freed exactly once when the returned closure is dropped by the caller.
#[test]
fn test_closure_returned_from_function() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn make_counter(items [int]) fn() int
    fn() int: items.length()

fn main()
    let result = make_counter(List([1, 2, 3]))()
    println(f"{result}")
"#,
        "3",
    );
}

/// Return a closure that captures a locally-created List (not a parameter).
#[test]
fn test_closure_returned_captures_local_list() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn make_counter() fn() int
    let items = List([1, 2, 3])
    fn() int: items.length()

fn main()
    let counter = make_counter()
    let result = counter()
    println(f"{result}")
"#,
        "3",
    );
}

/// Return a non-capturing closure from a function.
#[test]
fn test_closure_returned_no_capture() {
    assert_heap_guard_output(
        r#"

fn make_fn() fn() int
    fn() int: 42

fn main()
    let f = make_fn()
    let result = f()
    println(f"{result}")
"#,
        "42",
    );
}

/// Return a closure that captures a primitive parameter.
#[test]
fn test_closure_returned_captures_int_param() {
    assert_heap_guard_output(
        r#"

fn make_adder(n int) fn() int
    fn() int: n + 1

fn main()
    let f = make_adder(10)
    let result = f()
    println(f"{result}")
"#,
        "11",
    );
}

/// Debug: check list length inside make_counter vs after.
#[test]
fn test_debug_list_param_capture_length() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn make_counter(items [int]) fn() int
    println(f"inside:{items.length()}")
    fn() int: items.length()

fn main()
    let counter = make_counter(List([1, 2, 3]))
    let result = counter()
    println(f"outside:{result}")
"#,
        "inside:3\noutside:3",
    );
}

/// Closure captures a class instance that has a managed (List) field.
/// When the closure drops, the class's RC must reach 0 and the List field
/// must be freed via the drop thunk — not just via a bare libc::free.
#[test]
fn test_closure_captures_class_with_list_field() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Counter
    items [int]

fn make_reader(c Counter) fn() int
    fn() int: c.items.length()

fn main()
    let c = Counter(List([10, 20, 30]))
    let reader = make_reader(c)
    println(f"{reader()}")
"#,
        "3",
    );
}

/// Two independent lambdas each capture the same List; dropping both lambdas
/// must not double-free the List.
#[test]
fn test_two_lambdas_capture_same_list_no_double_free() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let pool = List([7, 8, 9])
    let f1 = fn() int: pool.length()
    let f2 = fn() int: pool.length()
    println(f"{f1() + f2()}")
"#,
        "6",
    );
}

/// Reassigning a variable that holds a capturing closure releases that closure,
/// and the drop thunk of its environment releases its captures: the capture is not
/// released a second time on top of that, whether the new closure captures or
/// not.
#[test]
fn test_reassigning_a_capturing_closure_releases_its_capture_once() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    let data = List([1, 2, 3])
    var f = fn() int: data.length()
    println(f"{f()}")
    f = fn() int: 99
    println(f"{f()}")
    f = fn() int: data.length() + 10
    println(f"{f()} {data.length()}")
"#,
        "3\n99\n13 3",
    );
}

/// A closure still held elsewhere keeps its captures when the variable it was
/// read from is reassigned.
#[test]
fn test_reassigning_a_shared_capturing_closure_keeps_its_capture_alive() {
    assert_heap_guard_output(
        r#"
fn main()
    let tag = f"tag{1}"
    var f = fn() int: tag.length()
    let keep = f
    f = fn() int: 0
    println(f"{keep()} {f()} {tag}")
"#,
        "4 0 tag1",
    );
}

/// A closure capturing a vector holds it in its environment the way the
/// vector is held anywhere, and dropping the closure leaves nothing behind.
#[test]
fn test_lambda_captures_vector_no_leak() {
    assert_heap_guard_output(
        r#"
use system.gpu.vector

fn main()
    let v = Vec3(1.0, 2.0, 3.0)
    let f = fn() float: v.x + v.z
    println(f"{f()} {v.y}")
"#,
        "4.0 2.0",
    );
}

/// A closure capturing a vector beside a string releases both when dropped.
#[test]
fn test_lambda_captures_vector_and_string_no_leak() {
    assert_heap_guard_output(
        r#"
use system.gpu.vector

fn main()
    let v = Vec3(1.0, 2.0, 3.0)
    let s = f"hello{1}"
    let f = fn() float:
        println(s)
        return v.y
    println(f"{f()}")
"#,
        "hello1\n2.0",
    );
}

const CLOSING_HANDLE: &str = r#"
class Handle
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")
"#;

fn with_closing_handle(program: &str) -> String {
    format!("{CLOSING_HANDLE}\n{program}")
}

/// A resource captured by a kept closure that is copied into a list and
/// returned is released once, when the list's copy, the last one, goes.
#[test]
fn a_resource_captured_by_a_closure_copied_into_a_returned_list_is_released_once() {
    assert_heap_guard_output(
        &with_closing_handle(
            r#"
use system.collections.list

fn make() [fn() int]
    let h = Handle(1)
    let f = fn() int: h.id
    var fs = List<fn() int>()
    fs.push(f)
    println(f"made {f()}")
    return fs

fn main()
    if true
        let fs = make()
        println(f"held {fs.length()}")
    println("done")
"#,
        ),
        "made 1\nheld 1\nclosed 1\ndone",
    );
}

/// Releasing a long chain of closures, each capturing the one before it,
/// does not take a native frame per link, and the resource at the bottom of
/// the chain is released once.
#[test]
fn a_deep_chain_of_capturing_closures_is_released_without_overflow() {
    assert_heap_guard_output(
        &with_closing_handle(
            r#"
fn main()
    if true
        let h = Handle(7)
        var f = fn() int: h.id
        var i = 0
        while i < 200000
            let g = f
            f = fn() int: g() + 1
            i = i + 1
        println("built")
    println("done")
"#,
        ),
        "built\nclosed 7\ndone",
    );
}

/// A closure capturing two resources releases them in the order it captured
/// them: an environment's captures are released forward, in capture order,
/// like an aggregate's fields.
#[test]
fn a_closure_capturing_two_resources_releases_them_in_capture_order() {
    assert_heap_guard_output(
        &with_closing_handle(
            r#"
fn main()
    if true
        let a = Handle(1)
        let b = Handle(2)
        let f = fn() int: a.id + b.id
        println(f"sum {f()}")
    println("done")
"#,
        ),
        "sum 3\nclosed 1\nclosed 2\ndone",
    );
}

/// A function specialized for a gpu-resident argument lowers its lambda a
/// second time, under the specialization's residency, so the two closures
/// each get a drop thunk of their own rather than one name defined twice.
#[test]
fn a_lambda_in_a_residency_specialized_function_has_its_own_drop_thunk() {
    assert_builds(
        r#"
use system.collections.array

fn scale(a out Array<int,8>)
    let tag = "scaled"
    let say = fn() String: tag
    let _said = say()
    forall i in 0..a.length()
        a[i] = a[i] * 2

fn main()
    gpu var data = [1, 2, 3, 4, 5, 6, 7, 8]
    scale(data)
    var host = [1, 2, 3, 4, 5, 6, 7, 8]
    scale(host)
    println(f"{host[1]}")
"#,
    );
}

/// A closure releases a resource held at every capture position, not only
/// the first: each slot of its environment is loaded and released once.
#[test]
fn a_closure_releases_a_resource_at_every_capture_position() {
    assert_heap_guard_output(
        &with_closing_handle(
            r#"
fn main()
    if true
        let a = Handle(1)
        let b = Handle(2)
        let c = Handle(3)
        let f = fn() int: a.id + b.id + c.id
        println(f"sum {f()}")
    println("done")
"#,
        ),
        "sum 6\nclosed 1\nclosed 2\nclosed 3\ndone",
    );
}

/// A 128-bit capture takes two words of the environment, and the counted
/// capture after it is stored, loaded and released from the slot past them.
#[test]
fn a_wide_scalar_capture_before_a_counted_one_keeps_both_intact() {
    assert_heap_guard_output(
        r#"
fn main()
    let wide i128 = 170141183460469231731687303715884105727
    let half = 2.5
    let tag = f"tag{1}"
    let f = fn() String: f"{wide == 170141183460469231731687303715884105727} {half} {tag}"
    println(f())
"#,
        "true 2.5 tag1",
    );
}

/// A bitwise struct is held in the environment as its bytes, and a counted
/// capture after it is laid out past all of them.
#[test]
fn a_struct_held_as_bytes_before_a_counted_capture_keeps_both_intact() {
    assert_heap_guard_output(
        r#"
struct Span3
    a int
    b int
    c int

fn main()
    let s = Span3(1, 2, 3)
    let tag = f"tag{4}"
    let f = fn() String: f"{s.a + s.b + s.c} {tag}"
    println(f())
"#,
        "6 tag4",
    );
}

/// One generic lambda instantiated at a scalar and at a resource gets an
/// environment for each: the resource's hook runs once, and the scalar
/// instance releases nothing.
#[test]
fn a_generic_lambda_at_a_scalar_and_at_a_resource_releases_each_correctly() {
    assert_heap_guard_output(
        &with_closing_handle(
            r#"
fn touch<T>(value T) int
    return 1

fn keep<T>(value T) fn() int
    return fn() int: touch(value)

fn main()
    if true
        let at_int = keep(5)
        let at_handle = keep(Handle(9))
        println(f"calls {at_int() + at_handle()}")
    println("done")
"#,
        ),
        "calls 2\nclosed 9\ndone",
    );
}

/// A closure kept in a class field is released with the object holding it,
/// and its captured resource with it.
#[test]
fn a_closure_kept_in_a_class_field_is_released_with_its_holder() {
    assert_heap_guard_output(
        &with_closing_handle(
            r#"
class Keeper
    public var callback (fn() int)?
    public fn init()
        self.callback = None

fn main()
    if true
        let h = Handle(4)
        var keeper = Keeper()
        keeper.callback = Some(fn() int: h.id)
        match keeper.callback
            Some(f): println(f"called {f()}")
            None: println("none")
    println("done")
"#,
        ),
        "called 4\nclosed 4\ndone",
    );
}

/// A closure returned inside a tuple is released with the tuple, and its
/// captured string with it.
#[test]
fn a_closure_returned_in_a_tuple_is_released_with_it() {
    assert_heap_guard_output(
        r#"
fn make() (int, fn() String)
    let tag = f"tag{2}"
    return (7, fn() String: tag)

fn main()
    let pair = make()
    let f = pair.1
    println(f"{pair.0} {f()}")
"#,
        "7 tag2",
    );
}
