// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Operands are evaluated left to right, and each yields its value at the
//! moment it is evaluated. A later operand that writes what an earlier one
//! read — through an `out` argument, an assignment, a method that writes its
//! receiver, or a write to an object whose field was read — leaves the
//! earlier value as it was. Each program prints `<value>` so the expected
//! value is matched whole.

use super::utils::*;

/// The functions every program below calls: `bumpi` sets its `out`
/// argument to 99 and returns 0, `refill` replaces its `out` list with
/// `[5, 6, 7, 8]` and returns 5, and `showi` prints its first argument.
const HELPERS: &str = r#"
use system.io
use system.collections.list

fn bumpi(n out int) int
    n = 99
    return 0

fn refill(xs out List<int>) int
    xs = List([5, 6, 7, 8])
    return 5

fn showi(a int, _b int)
    println(f"<{a}>")
"#;

fn program(rest: &str) -> String {
    format!("{HELPERS}\n{rest}")
}

#[test]
fn an_argument_keeps_its_value_when_a_later_argument_writes_it_through_out() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var n = 1
    showi(n, bumpi(n))
"#,
        ),
        "<1>",
    );
}

#[test]
fn an_argument_passed_again_as_the_same_calls_out_argument_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
fn g(a List<int>, b out List<int>) int
    b = List([9, 9, 9])
    return a.length()

fn main()
    var n = List([1])
    println(f"<{g(n, n)}>")
"#,
        ),
        "<1>",
    );
}

#[test]
fn an_operators_left_side_keeps_its_value_when_the_right_side_writes_it() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var n = 1
    println(f"<{n + bumpi(n)}>")
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_string_concatenations_left_side_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
fn news(s out String) String
    s = "zz"
    return "b"

fn main()
    var s = "a"
    println(f"<{s + news(s)}>")
"#,
        ),
        "<ab>",
    );
}

#[test]
fn a_string_comparisons_left_side_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
fn news(s out String) String
    s = "zz"
    return "b"

fn main()
    var s = "a"
    println(f"<{s < news(s)}>")
"#,
        ),
        "<true>",
    );
}

#[test]
fn a_membership_tests_left_side_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
use system.collections.set

fn main()
    var k = 1
    println(f"<{k in Set({1, bumpi(k)})}>")
"#,
        ),
        "<true>",
    );
}

#[test]
fn a_struct_field_argument_keeps_its_value_when_the_struct_is_written_through_out() {
    assert_runs_with_output(
        &program(
            r#"
struct Sphere
    center float
    radius float

fn bump(s out Sphere) int
    s = Sphere(center: 99.0, radius: 2.0)
    return 0

fn show(c float, _b int)
    println(f"<{c}>")

fn main()
    var s = Sphere(center: 1.5, radius: 1.0)
    show(s.center, bump(s))
"#,
        ),
        "<1.5>",
    );
}

#[test]
fn a_receiver_keeps_its_value_when_an_argument_writes_it_through_out() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var zs = List([1, 2])
    println(f"<{zs.contains(refill(zs))}>")
"#,
        ),
        "<false>",
    );
}

#[test]
fn a_class_field_argument_keeps_its_value_when_a_method_writes_the_object() {
    assert_runs_with_output(
        &program(
            r#"
class Box
    var v int
    fn set(x int) int
        self.v = x
        return 0

fn main()
    let c = Box(v: 1)
    showi(c.v, c.set(50))
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_class_field_argument_keeps_its_value_when_a_function_writes_the_object() {
    assert_runs_with_output(
        &program(
            r#"
class Box
    var v int
    fn set(x int) int
        self.v = x
        return 0

fn poke(b Box) int
    return b.set(99)

fn main()
    let b = Box(v: 1)
    showi(b.v, poke(b))
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_field_of_self_keeps_its_value_when_a_method_of_self_writes_it() {
    assert_runs_with_output(
        &program(
            r#"
class Box
    var v int
    fn set(x int) int
        self.v = x
        return 0
    fn report()
        showi(self.v, self.set(7))

fn main()
    let b = Box(v: 1)
    b.report()
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_tuple_element_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var k = 1
    let t = (k, bumpi(k))
    println(f"<{t.0}>")
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_tuple_element_list_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var xs = List([1])
    let t = (xs, refill(xs))
    println(f"<{t.0.length()}>")
"#,
        ),
        "<1>",
    );
}

#[test]
fn an_array_element_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var q = 1
    let a = [q, bumpi(q)]
    println(f"<{a[0]}>")
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_list_element_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var d = 1
    let l = List([d, bumpi(d)])
    println(f"<{l[0]}>")
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_struct_constructor_argument_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
struct P
    x int
    y int

fn main()
    var a = 1
    let p = P(x: a, y: bumpi(a))
    println(f"<{p.x}>")
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_class_constructor_argument_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
class C
    var a int
    var b int

fn main()
    var b = 1
    let c = C(a: b, b: bumpi(b))
    println(f"<{c.a}>")
"#,
        ),
        "<1>",
    );
}

#[test]
fn an_enum_payload_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
enum Pair
    Two(int, int)

fn main()
    var n = 1
    let p = Pair.Two(n, bumpi(n))
    match p
        Pair.Two(a, _): println(f"<{a}>")
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_list_argument_keeps_its_value_when_a_later_argument_replaces_it() {
    assert_runs_with_output(
        &program(
            r#"
fn len2(a List<int>, _b int) int
    return a.length()

fn main()
    var xs = List([1])
    println(f"<{len2(xs, refill(xs))}>")
"#,
        ),
        "<1>",
    );
}

#[test]
fn an_indexed_value_is_read_before_its_index_writes_it() {
    assert_runs_with_output(
        &program(
            r#"
fn idx(xs out List<int>) int
    xs = List([7, 8, 9])
    return 1

fn main()
    var zs = List([10, 20, 30])
    println(f"<{zs[idx(zs)]}>")
"#,
        ),
        "<20>",
    );
}

#[test]
fn an_element_argument_keeps_its_value_when_a_later_argument_pops_the_list() {
    assert_runs_with_output(
        &program(
            r#"
fn show_opt(a int, _b int?)
    println(f"<{a}>")

fn main()
    var xs = List([1, 2, 3])
    show_opt(xs[2], xs.pop())
"#,
        ),
        "<3>",
    );
}

#[test]
fn an_element_argument_keeps_its_value_when_a_later_argument_writes_its_index() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var xs = List([10, 20, 30])
    var i = 0
    showi(xs[i], bumpi(i))
"#,
        ),
        "<10>",
    );
}

#[test]
fn a_range_start_keeps_its_value_when_its_end_writes_it() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var k = 1
    for x in k..bumpi(k) + 3
        println(f"<{x}>")
"#,
        ),
        "<1>\n<2>",
    );
}

#[test]
fn a_range_value_start_keeps_its_value_when_its_end_writes_it() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var k = 1
    let r = k..bumpi(k) + 3
    for x in r
        println(f"<{x}>")
"#,
        ),
        "<1>\n<2>",
    );
}

#[test]
fn an_argument_keeps_its_value_when_a_later_argument_assigns_it() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var n = 1
    showi(n, (n = 5))
"#,
        ),
        "<1>",
    );
}

#[test]
fn an_argument_keeps_its_value_when_a_match_arm_of_a_later_argument_assigns_it() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var n = 1
    let k = 2
    showi(n, (match k: 2: bumpi(n), _: 0))
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_math_intrinsic_argument_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
use system.math

fn main()
    var a = 1
    println(f"<{max(a, bumpi(a))}>")
"#,
        ),
        "<1>",
    );
}

#[test]
fn an_assertions_actual_value_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
use system.testing

fn main()
    var a = 1
    assert_eq(a, bumpi(a) + 1)
    println(f"<{a}>")
"#,
        ),
        "<99>",
    );
}

#[test]
fn a_string_argument_keeps_its_value_when_a_later_argument_replaces_it() {
    assert_runs_with_output(
        &program(
            r#"
fn news(s out String) String
    s = "zz"
    return "b"

fn shows(a String, b String)
    println(f"<{a}{b}>")

fn main()
    var s = "a"
    shows(s, news(s))
"#,
        ),
        "<ab>",
    );
}

#[test]
fn a_string_enum_payload_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
enum Pair
    Two(String, String)

fn news(s out String) String
    s = "zz"
    return "b"

fn main()
    var s = "a"
    let p = Pair.Two(s, news(s))
    match p
        Pair.Two(a, b): println(f"<{a}{b}>")
"#,
        ),
        "<ab>",
    );
}

#[test]
fn a_string_field_of_a_class_keeps_its_value_when_a_method_renames_the_object() {
    assert_runs_with_output(
        &program(
            r#"
class Named
    var name String
    fn rename(to String) String
        self.name = to
        return "!"

fn shows(a String, b String)
    println(f"<{a}{b}>")

fn main()
    let n = Named(name: "old")
    shows(n.name, n.rename("new"))
    println(f"[{n.name}]")
"#,
        ),
        "<old!>\n[new]",
    );
}

#[test]
fn a_string_tuple_element_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
fn news(s out String) String
    s = "zz"
    return "b"

fn main()
    var s = "a"
    let t = (s, news(s))
    println(f"<{t.0}{t.1}>")
"#,
        ),
        "<ab>",
    );
}

#[test]
fn a_list_sets_index_keeps_its_value_when_the_stored_value_writes_it() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var xs = List([10, 20, 30])
    var j = 1
    xs.set(j, bumpi(j))
    println(f"<{xs[1]} {j}>")
"#,
        ),
        "<0 99>",
    );
}

#[test]
fn a_list_inserts_index_keeps_its_value_when_the_inserted_value_writes_it() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var ys = List([10, 20, 30])
    var k = 1
    ys.insert(k, bumpi(k))
    println(f"<{ys.length()} {ys[1]} {k}>")
"#,
        ),
        "<4 0 99>",
    );
}

#[test]
fn a_list_sets_index_out_of_range_is_reported_at_the_value_it_was_evaluated_at() {
    assert_runtime_error(
        &program(
            r#"
fn main()
    var xs = List([10, 20, 30])
    var j = 7
    xs.set(j, bumpi(j))
"#,
        ),
        "index is 7",
    );
}

/// `Box` and `Holder`, for the programs reading a field of an object no
/// binding names.
const BOXES: &str = r#"
class Box
    var v int
    var s String
    fn set(x int) int
        self.v = x
        return 0
    fn rename(x String) int
        self.s = x
        return 0

class Holder
    var b Box
    fn get() Box
        return self.b
    fn find() Box?
        return self.b

fn shows(a String, _b int)
    println(f"<{a}>")
"#;

fn boxes(rest: &str) -> String {
    program(&format!("{BOXES}\n{rest}"))
}

#[test]
fn a_field_of_a_call_result_keeps_its_value_when_a_later_argument_writes_the_object() {
    assert_runs_with_output(
        &boxes(
            r#"
fn main()
    let h = Holder(b: Box(v: 1, s: "a"))
    showi(h.get().v, h.b.set(9))
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_field_of_a_conditional_keeps_its_value_when_a_later_argument_writes_the_object() {
    assert_runs_with_output(
        &boxes(
            r#"
fn main()
    let b = Box(v: 1, s: "a")
    let c = true
    showi((if c: b else: b).v, b.set(9))
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_field_of_a_coalesced_optional_keeps_its_value_when_a_later_argument_writes_the_object() {
    assert_runs_with_output(
        &boxes(
            r#"
fn main()
    let h = Holder(b: Box(v: 1, s: "a"))
    showi((h.find() ?? Box(v: 5, s: "b")).v, h.b.set(9))
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_string_field_of_a_call_result_keeps_its_value_when_a_later_argument_replaces_it() {
    assert_heap_guard_output(
        &boxes(
            r#"
fn main()
    let h = Holder(b: Box(v: 1, s: "old" + "x"))
    shows(h.get().s, h.b.rename("new" + "y"))
    println(f"[{h.b.s}]")
"#,
        ),
        "<oldx>\n[newy]",
    );
}

#[test]
fn a_list_membership_tests_left_side_is_evaluated_before_the_list() {
    assert_runs_with_output(
        &program(
            r#"
fn main()
    var k = 1
    println(f"<{k in List([1, bumpi(k)])}>")
"#,
        ),
        "<true>",
    );
}

#[test]
fn an_array_membership_tests_left_side_is_evaluated_before_the_array() {
    assert_runs_with_output(
        &program(
            r#"
use system.collections.array

fn main()
    var k = 1
    println(f"<{k in [1, bumpi(k)]}>")
"#,
        ),
        "<true>",
    );
}

#[test]
fn a_string_membership_tests_left_side_is_evaluated_before_the_string() {
    assert_heap_guard_output(
        &program(
            r#"
fn nstr(s out String) String
    s = "zz"
    return "tail"

fn main()
    var u = "needle"
    println(f"<{u in 'zzz' + nstr(u)}>")
"#,
        ),
        "<false>",
    );
}

#[test]
fn a_membership_tests_left_side_runs_before_the_collection_is_built() {
    assert_runs_with_output(
        &program(
            r#"
fn note(s String, v int) int
    println(s)
    return v

fn main()
    println(f"<{note('left', 1) in List([note('right', 1)])}>")
"#,
        ),
        "left\nright\n<true>",
    );
}

#[test]
fn an_indexed_map_is_read_before_its_key_replaces_it() {
    assert_heap_guard_output(
        &program(
            r#"
use system.collections.map

fn reset(m out Map<int, String>) int
    m = {1: "new" + "a", 2: "new" + "b"}
    return 1

fn main()
    var m = {1: "old" + "a", 2: "old" + "b"}
    let v = m[reset(m)]
    println(f"<{v}> <{m[1]}>")
    println(f"<{m[reset(m)]}>")
"#,
        ),
        "<olda> <newa>\n<newa>",
    );
}

#[test]
fn an_indexed_map_of_ints_is_read_before_its_key_replaces_it() {
    assert_runs_with_output(
        &program(
            r#"
use system.collections.map

fn reset(m out Map<int, int>) int
    m = {1: 500, 2: 600}
    return 1

fn main()
    var m = {1: 10, 2: 20}
    println(f"<{m[reset(m)]}>")
"#,
        ),
        "<10>",
    );
}

#[test]
fn named_arguments_out_of_order_keep_the_value_each_was_evaluated_at() {
    assert_runs_with_output(
        &program(
            r#"
fn second(a int, b int)
    println(f"<{a} {b}>")

fn main()
    var n = 1
    second(b: n, a: bumpi(n))
"#,
        ),
        "<0 1>",
    );
}

#[test]
fn a_static_methods_argument_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
class K
    static fn show(a int, _b int)
        println(f"<{a}>")

fn main()
    var n = 1
    K.show(n, bumpi(n))
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_super_calls_argument_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
class Base
    var v int
    fn show(a int, _b int)
        println(f"<{a}>")

class Child extends Base
    fn show(a int, b int)
        var n = a
        super.show(n, bumpi(n))

fn main()
    let c = Child(v: 0)
    c.show(1, 2)
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_trait_typed_receivers_argument_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
trait Shows
    fn show(a int, b int)

class Shower implements Shows
    var v int
    fn show(a int, _b int)
        println(f"<{a}>")

fn run(s Shows)
    var n = 1
    s.show(n, bumpi(n))

fn main()
    run(Shower(v: 0))
"#,
        ),
        "<1>",
    );
}

#[test]
fn a_generic_argument_bound_to_a_class_keeps_its_value() {
    assert_runs_with_output(
        &program(
            r#"
class Box
    var v int

fn keep<T>(a T, _b int) T: a

fn swap<T>(x out T, y T) int
    x = y
    return 0

fn first<T>(x T, y T) T
    var t = x
    return keep(t, swap(t, y))

fn main()
    let r = first(Box(v: 1), Box(v: 2))
    println(f"<{r.v}>")
"#,
        ),
        "<1>",
    );
}

#[test]
fn an_argument_keeps_its_value_when_a_nested_call_writes_it() {
    assert_runs_with_output(
        &program(
            r#"
fn id(x int) int: x

fn main()
    var n = 1
    showi(n, id(bumpi(n)))
"#,
        ),
        "<1>",
    );
}

#[test]
fn an_indexed_maps_snapshot_lives_until_its_borrowed_entry_is_used_in_a_loop() {
    assert_heap_guard_output(
        &program(
            r#"
use system.collections.map

fn reset(m out Map<int, String>) int
    m = {1: "new" + "a", 2: "new" + "b"}
    return 1

fn first_long(m out Map<int, String>) String
    for i in 0..3
        let s = f"{m[reset(m)]}"
        if i == 1
            return s
    return "none"

fn main()
    var m = {1: "old" + "a", 2: "old" + "b"}
    for _ in 0..2
        println(f"<{m[reset(m)]}>")
    println(first_long(m))
"#,
        ),
        "<olda>\n<newa>\nnewa",
    );
}

/// A snapshot of an indexed map taken in a loop is released on every way
/// out of the iteration that took it: running on, `continue`, `break` and a
/// `return` from inside the loop, ten thousand times over.
#[test]
fn an_indexed_maps_snapshot_is_released_on_every_exit_from_a_loop() {
    assert_heap_guard_output(
        &program(
            r#"
use system.collections.map

fn reset(m out Map<int, String>) int
    m = {1: "new" + "a", 2: "new" + "b"}
    return 1

fn count(m out Map<int, String>, stop int) int
    var total = 0
    for i in 0..10000
        let s = f"{m[reset(m)]}"
        if i % 3 == 0
            continue
        if i == stop
            break
        if i == 9001
            return total + s.length()
        total = total + s.length()
    return total

fn main()
    var m = {1: "old" + "a", 2: "old" + "b"}
    println(f"<{count(m, 5000)}> <{count(m, 20000)}>")
"#,
        ),
        "<13332> <24004>",
    );
}

/// A `String` or class instance tested for membership in a list or a
/// string a later operand builds after writing it, as a whole binding and as
/// a field, keeps the value it was evaluated at, and the copy taken of it is
/// released once: the heap guard sees no double release and no leak.
#[test]
fn a_counted_left_side_of_in_is_held_and_released_once() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Box
    var v int

class Holder
    var name String
    var who Box

    fn rename() List<String>
        self.name = "zz" + "z"
        return List(["ab" + ""])

    fn retitle() String
        self.name = "zz" + "z"
        return "xx" + "ab"

    fn swap() List<Box>
        let old = self.who
        self.who = Box(v: 2)
        return List([old])

fn pick(s out String) List<String>
    s = "zz" + "z"
    return List(["ab" + ""])

fn grow(s out String) String
    s = "zz" + "z"
    return "xx" + "ab"

fn boxes(b out Box) List<Box>
    let old = b
    b = Box(v: 2)
    return List([old])

fn main()
    var s = "a" + "b"
    let a = s in pick(s)
    var t = "a" + "b"
    let c = t in grow(t)
    var b = Box(v: 1)
    let d = b in boxes(b)
    var h = Holder(name: "a" + "b", who: Box(v: 1))
    let e = h.name in h.rename()
    h.name = "a" + "b"
    let f = h.name in h.retitle()
    let g = h.who in h.swap()
    println(f"<{a} {c} {d} {e} {f} {g}> {s} {t} {b.v} {h.name} {h.who.v}")
"#,
        "<true true true true true true> zzz zzz 2 zzz 2",
    );
}

/// An enum's own `equals` runs where `==` is evaluated, so a field it writes
/// through the class instance a variant holds was read before it, by the
/// earlier operand.
#[test]
fn an_operand_keeps_its_value_when_an_enums_operator_method_writes_it_later() {
    assert_runs_with_output(
        "
use system.io

class Box
    var v int

    fn bump()
        self.v = 99

enum Tag
    Plain
    Boxed(Box)

    fn equals(other Self) bool
        match self
            Tag.Boxed(b):
                b.bump()
                true
            Tag.Plain: false

struct Pair
    left int
    same bool

fn main()
    let b = Box(v: 1)
    let t = Tag.Boxed(b)
    let p = Pair(left: b.v, same: t == Tag.Plain)
    println(f\"<{p.left}> {p.same} {b.v}\")
",
        "<1> true 99",
    );
}

/// A struct compared field by field runs the `equals` a field's enum
/// declares, which writes the earlier operand's object.
#[test]
fn an_operand_keeps_its_value_when_a_structs_comparison_runs_a_method_later() {
    assert_runs_with_output(
        "
use system.io

class Box
    var v int

    fn bump()
        self.v = 99

enum Tag
    Plain
    Boxed(Box)

    fn equals(other Self) bool
        match self
            Tag.Boxed(b):
                b.bump()
                true
            Tag.Plain: false

struct Wrap
    t Tag

struct Pair
    left int
    same bool

fn main()
    let b = Box(v: 1)
    let w = Wrap(t: Tag.Boxed(b))
    let plain = Wrap(t: Tag.Plain)
    let p = Pair(left: b.v, same: w == plain)
    println(f\"<{p.left}> {p.same} {b.v}\")
",
        "<1> true 99",
    );
}

/// A generic enum compared variant by variant reads its payloads at the
/// arguments it is instantiated with, so a payload whose enum declares an
/// `equals` that writes the earlier operand's object runs there, after that
/// operand was read.
#[test]
fn an_operand_keeps_its_value_when_a_generic_enums_payload_comparison_runs_a_method() {
    assert_runs_with_output(
        r#"
use system.io

class Box
    var v int

    fn bump()
        self.v = 99

enum Tag
    Plain
    Boxed(Box)

    fn equals(_other Self) bool
        match self
            Tag.Boxed(b):
                b.bump()
                true
            Tag.Plain: false

enum Slot<T>
    Empty
    Full(T)

struct Pair
    left int
    same bool

fn main()
    let b = Box(v: 1)
    let s = Slot<Tag>.Full(Tag.Boxed(b))
    let other = Slot<Tag>.Full(Tag.Plain)
    let p = Pair(left: b.v, same: s == other)
    println(f"<{p.left}> {p.same} {b.v}")
"#,
        "<1> true 99",
    );
}
