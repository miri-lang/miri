// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A function whose parameter is only bound to a local takes its argument
//! over: the caller funds one reference for the call, and where the argument
//! dies at the call that reference is the caller's own, so the callee holds the
//! value alone and writes it in place. Wherever the caller still needs the
//! value — an alias, a later read, the same value in two arguments — the
//! callee's write copies first, and the caller sees what it had.

use super::super::utils::{
    assert_heap_guard_output, assert_runs_with_output, get_allocation_count_value,
};

/// The callee every program below shares: it binds its parameter, then writes
/// every element of that binding.
const ADVANCE: &str = r#"
use system.io
use system.collections.list

fn advance(start [int], step int) [int]
    var items = start
    for i in 0..items.length():
        items[i] = items[i] + step
    return items
"#;

fn with_advance(main: &str) -> String {
    format!("{ADVANCE}\n{main}")
}

/// A simulation step written as a function over a list of structs, iterated
/// `STEPS` times, each result replacing the list it was computed from.
const STEPPED_BODIES: &str = r#"
use system.io
use system.collections.list

struct Body
    x float
    vx float

fn advance(start [Body], dt float) [Body]
    var bodies = start
    for i in 0..bodies.length():
        bodies[i].vx = bodies[i].vx + dt
        bodies[i].x = bodies[i].x + bodies[i].vx * dt
    return bodies

fn main()
    var bodies = List([Body(x: 0.0, vx: 1.0), Body(x: 1.0, vx: 0.0)])
    for s in 0..STEPS:
        bodies = advance(bodies, 0.01)
    println(f"{bodies[0].x}")
"#;

fn stepped_bodies(steps: usize) -> String {
    STEPPED_BODIES.replace("STEPS", &steps.to_string())
}

#[test]
fn test_a_list_replaced_by_its_update_allocates_the_same_at_any_step_count() {
    let few = get_allocation_count_value(&stepped_bodies(10));
    let many = get_allocation_count_value(&stepped_bodies(1000));
    assert_eq!(
        few, many,
        "each step copied the list it was handed: {few} allocations at 10 steps, {many} at 1000"
    );
    assert_runs_with_output(&stepped_bodies(1000), "60.049999999999145");
}

/// The caller keeps an alias of the argument it hands over, so the callee finds
/// the value shared and copies it before writing. Guards against the caller
/// releasing the alias's reference along with the one it handed over.
#[test]
fn test_an_alias_the_caller_keeps_is_unchanged_by_the_update() {
    assert_runs_with_output(
        &with_advance(
            r#"
fn main()
    var items = List([1, 2, 3])
    let keep = items
    items = advance(items, 10)
    println(f"keep {keep[0]} {keep[1]} {keep[2]} items {items[0]} {items[1]} {items[2]}")
"#,
        ),
        "keep 1 2 3 items 11 12 13",
    );
}

#[test]
fn test_an_argument_read_after_the_call_is_unchanged() {
    assert_runs_with_output(
        &with_advance(
            r#"
fn main()
    let items = List([1, 2, 3])
    let moved = advance(items, 10)
    println(f"items {items[0]} moved {moved[0]}")
"#,
        ),
        "items 1 moved 11",
    );
}

#[test]
fn test_one_value_passed_twice_keeps_the_borrowed_side_unchanged() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn both(first [int], second [int]) int
    var mine = first
    mine[0] = 100
    return mine[0] + second[0]

fn each(first [int], second [int]) int
    var one = first
    var two = second
    one[0] = 100
    two[0] = 200
    return one[0] + two[0]

fn main()
    let l = List([1, 2])
    println(f"both {both(l, l)} each {each(l, l)} l {l[0]}")
"#,
        "both 101 each 300 l 1",
    );
}

#[test]
fn test_a_call_repeated_in_a_loop_sees_the_original_every_time() {
    assert_runs_with_output(
        &with_advance(
            r#"
fn main()
    let items = List([1, 2, 3])
    var total = 0
    for s in 0..3:
        let moved = advance(items, s)
        total = total + moved[0]
    println(f"total {total} items {items[0]}")
"#,
        ),
        "total 6 items 1",
    );
}

/// A recursive function handing its own updated binding to the next level.
const RECURSIVE_FILL: &str = r#"
use system.io
use system.collections.list

fn fill(start [int], n int) [int]
    var items = start
    if n == 0
        return items
    items[0] = items[0] + n
    return fill(items, n - 1)

fn main()
    let filled = fill(List([0, 0]), DEPTH)
    println(f"filled {filled[0]}")
"#;

fn recursive_fill(depth: usize) -> String {
    RECURSIVE_FILL.replace("DEPTH", &depth.to_string())
}

#[test]
fn test_a_recursive_update_allocates_the_same_at_any_depth() {
    let shallow = get_allocation_count_value(&recursive_fill(10));
    let deep = get_allocation_count_value(&recursive_fill(100));
    assert_eq!(
        shallow, deep,
        "each level copied the list: {shallow} allocations at depth 10, {deep} at depth 100"
    );
    assert_runs_with_output(&recursive_fill(100), "filled 5050");
}

#[test]
fn test_a_call_through_a_function_reference_updates_the_argument() {
    assert_runs_with_output(
        &with_advance(
            r#"
fn main()
    let f = advance
    var items = List([1, 2, 3])
    let keep = items
    items = f(items, 10)
    println(f"keep {keep[0]} items {items[0]} {items[2]}")
"#,
        ),
        "keep 1 items 11 13",
    );
}

#[test]
fn test_a_temporary_argument_is_updated() {
    assert_runs_with_output(
        &with_advance(
            r#"
fn main()
    let moved = advance(List([1, 2, 3]), 10)
    println(f"moved {moved[0]} {moved[2]}")
"#,
        ),
        "moved 11 13",
    );
}

/// An argument read out of a field is funded by a retain of its own, so the
/// struct keeps its reference. Guards against the call releasing the field's
/// reference along with the one it hands over.
#[test]
fn test_a_field_argument_leaves_the_field_unchanged() {
    assert_runs_with_output(
        &with_advance(
            r#"
struct Holder
    items [int]

fn main()
    let h = Holder(items: List([1, 2, 3]))
    let moved = advance(h.items, 10)
    println(f"field {h.items[0]} moved {moved[0]}")
"#,
        ),
        "field 1 moved 11",
    );
}

#[test]
fn test_a_string_literal_argument_is_extended_by_the_callee() {
    assert_runs_with_output(
        r#"
use system.io

fn shout(text String) String
    var loud = text
    loud = loud + "!"
    return loud

fn main()
    let greeting = "hi"
    println(f"{shout('hey')} {shout(greeting)} {greeting}")
"#,
        "hey! hi! hi",
    );
}

#[test]
fn test_an_early_return_hands_back_the_argument_unchanged() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn zero_first(start [int], keep bool) [int]
    var items = start
    if keep
        return items
    items[0] = 0
    return items

fn main()
    let items = List([5, 6])
    let kept = zero_first(items, true)
    let zeroed = zero_first(items, false)
    var temp = List([7, 8])
    temp = zero_first(temp, false)
    println(f"kept {kept[0]} zeroed {zeroed[0]} items {items[0]} temp {temp[0]} {temp[1]}")
"#,
        "kept 5 zeroed 0 items 5 temp 0 8",
    );
}

#[test]
fn test_a_generic_function_keeps_its_parameter_borrowed_for_int_and_string_lists() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn replace_first<T>(start [T], value T) [T]
    var items = start
    items[0] = value
    return items

fn main()
    let numbers = List([1, 2])
    var replaced = replace_first(numbers, 9)
    replaced = replace_first(replaced, 8)
    let words = List(["x", "y"])
    var swapped = replace_first(words, "z")
    swapped = replace_first(swapped, "w")
    println(f"numbers {numbers[0]} {replaced[0]} words {words[0]} {swapped[0]} {swapped[1]}")
"#,
        "numbers 1 8 words x w y",
    );
}

#[test]
fn test_a_parameter_with_a_drop_hook_is_dropped_where_it_was() {
    assert_runs_with_output(
        r#"
use system.io

class Res
    var name String

    fn init(name String)
        self.name = name

    fn drop(self)
        println(f"drop {self.name}")

fn hold(r Res) String
    let mine = r
    return mine.name

fn main()
    let r = Res("a")
    println(hold(r))
    println("after")
"#,
        "a\nafter\ndrop a",
    );
}

#[test]
fn test_a_method_binding_its_parameter_leaves_the_argument_unchanged() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class Stepper
    var step int

    fn init(step int)
        self.step = step

    fn advance(start [int]) [int]
        var items = start
        for i in 0..items.length():
            items[i] = items[i] + self.step
        return items

fn main()
    let stepper = Stepper(10)
    let items = List([1, 2])
    var moved = stepper.advance(items)
    moved = stepper.advance(moved)
    println(f"items {items[0]} moved {moved[0]} {moved[1]}")
"#,
        "items 1 moved 21 22",
    );
}

#[test]
fn test_a_list_of_values_with_a_drop_hook_is_dropped_where_it_was() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

class R
    var id int

    fn init(id int)
        self.id = id

    fn drop(self)
        println(f"drop R {self.id}")

enum Token
    Held(int)

    fn drop(self)
        println("drop Token")

fn consume(start [R]) int
    let token = Token.Held(99)
    var items = start
    println("in callee")
    match token
        Token.Held(v): return items.length() + v

fn main()
    let n = consume(List([R(1)]))
    println(f"after {n}")
"#,
        "in callee\ndrop Token\ndrop R 1\nafter 100",
    );
}

/// An n-body step: every pair of bodies pulls on each other by the inverse
/// square of their distance, then every body moves by its velocity. `NAME` is
/// the function's name, so one program can hold two copies of it.
const NBODY_STEP: &str = r#"
fn NAME(start [Body], dt float) [Body]
    var bodies = start
    let n = bodies.length()
    for i in 0..n:
        for j in i + 1..n:
            let dx = bodies[i].x - bodies[j].x
            let dy = bodies[i].y - bodies[j].y
            let d2 = dx * dx + dy * dy
            let mag = dt / (d2 * sqrt(d2))
            let mi = bodies[i].mass * mag
            let mj = bodies[j].mass * mag
            bodies[i].vx = bodies[i].vx - dx * mj
            bodies[i].vy = bodies[i].vy - dy * mj
            bodies[j].vx = bodies[j].vx + dx * mi
            bodies[j].vy = bodies[j].vy + dy * mi
    for i in 0..n:
        bodies[i].x = bodies[i].x + dt * bodies[i].vx
        bodies[i].y = bodies[i].y + dt * bodies[i].vy
    return bodies
"#;

/// The bodies, how they start, and the checksum a run reports.
const NBODY_SYSTEM: &str = r#"
use system.io
use system.math
use system.collections.list

struct Body
    x float
    y float
    vx float
    vy float
    mass float

fn initial_bodies() [Body]
    List([
        Body(x: 0.0, y: 0.0, vx: 0.0, vy: 0.0, mass: 10.0),
        Body(x: 1.0, y: 0.0, vx: 0.0, vy: 3.0, mass: 0.1),
        Body(x: -2.0, y: 0.5, vx: 0.5, vy: -2.0, mass: 0.2),
        Body(x: 0.0, y: 3.0, vx: -1.5, vy: 0.0, mass: 0.05)
    ])

fn checksum(bodies [Body]) float
    var sum = 0.0
    for b in bodies
        sum = sum + b.x + b.y
    return sum
"#;

/// The checksum 1000 steps of 0.001 reach.
const NBODY_CHECKSUM: &str = "-0.8649227495579224";

/// `steps` n-body steps, each result replacing the list it was computed from.
pub(crate) fn nbody_owned(steps: usize) -> String {
    format!(
        "{NBODY_SYSTEM}\n{}\nfn main()\n    var bodies = initial_bodies()\n    for s in 0..{steps}:\n        bodies = advance(bodies, 0.001)\n    println(f\"checksum {{checksum(bodies)}}\")\n",
        NBODY_STEP.replace("NAME", "advance"),
    )
}

#[test]
fn test_an_nbody_step_allocates_the_same_at_any_step_count() {
    let few = get_allocation_count_value(&nbody_owned(10));
    let many = get_allocation_count_value(&nbody_owned(1000));
    assert_eq!(
        few, many,
        "each step copied the bodies: {few} allocations at 10 steps, {many} at 1000"
    );
    assert_runs_with_output(&nbody_owned(1000), &format!("checksum {NBODY_CHECKSUM}\n"));
}

/// A copy of the step named as a value keeps its parameter borrowed — every
/// call through it copies the bodies before writing — and reaches the same
/// checksum as the step that takes its argument over.
#[test]
fn test_an_nbody_step_taking_its_argument_over_matches_one_borrowing_it() {
    let program = format!(
        "{NBODY_SYSTEM}\n{}\n{}\n{}",
        NBODY_STEP.replace("NAME", "advance"),
        NBODY_STEP.replace("NAME", "advance_lent"),
        r#"
fn main()
    let lent_step = advance_lent
    var owned = initial_bodies()
    var lent = initial_bodies()
    for s in 0..1000:
        owned = advance(owned, 0.001)
        lent = lent_step(lent, 0.001)
    println(f"owned {checksum(owned)} lent {checksum(lent)}")
"#
    );
    assert_runs_with_output(
        &program,
        &format!("owned {NBODY_CHECKSUM} lent {NBODY_CHECKSUM}\n"),
    );
}

#[test]
fn test_a_parameter_read_beside_one_taken_over_is_unchanged() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn scale(factors [int], start [int]) [int]
    var items = start
    for i in 0..items.length():
        items[i] = items[i] * factors[i]
    return items

fn main()
    let factors = List([2, 3])
    var items = List([5, 7])
    items = scale(factors, items)
    items = scale(factors, items)
    println(f"factors {factors[0]} {factors[1]} items {items[0]} {items[1]}")
"#,
        "factors 2 3 items 20 63",
    );
}

#[test]
fn test_a_set_taken_over_is_extended_and_an_alias_keeps_its_elements() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set

fn extend(start {int}, value int) {int}
    var items = start
    items.add(value)
    return items

fn main()
    let keep = {1, 2}
    var items = keep
    items = extend(items, 3)
    var fresh = {5}
    fresh = extend(fresh, 6)
    println(f"keep {keep.length()} {keep.contains(3)} items {items.length()} {items.contains(3)} fresh {fresh.length()}")
"#,
        "keep 2 false items 3 true fresh 2",
    );
}

#[test]
fn test_a_map_taken_over_is_written_and_an_alias_keeps_its_entries() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

fn put(start {String: int}, key String, value int) {String: int}
    var entries = start
    entries[key] = value
    return entries

fn main()
    let keep = {"a": 1}
    var entries = keep
    entries = put(entries, "a", 10)
    entries = put(entries, "b", 20)
    let kept = keep["a"]
    let a = entries["a"]
    let b = entries["b"]
    println(f"keep {kept} {keep.length()} entries {a} {b} {entries.length()}")
"#,
        "keep 1 1 entries 10 20 2",
    );
}

#[test]
fn test_an_optional_list_taken_over_is_replaced_and_none_passes_through() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn first_or(start [int]?, fallback int) int
    var held = start
    match held
        Some(items): return items[0]
        None: return fallback

fn bumped(start [int]?) [int]?
    var held = start
    match held
        Some(items)
            var copy = items
            copy[0] = copy[0] + 1
            held = Some(copy)
        None
            held = None
    return held

fn main()
    let keep = Some(List([1, 2]))
    let raised = bumped(keep)
    let missing = bumped(None)
    println(f"keep {first_or(keep, -1)} raised {first_or(raised, -1)} missing {first_or(missing, -1)} none {first_or(None, 7)}")
"#,
        "keep 1 raised 2 missing -1 none 7",
    );
}

#[test]
fn test_a_parameter_bound_inside_a_loop_or_branch_stays_borrowed_and_unchanged() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn in_loop(start [int], rounds int) int
    var total = 0
    for r in 0..rounds:
        var items = start
        items[0] = items[0] + r
        total = total + items[0]
    return total

fn in_branch(start [int], write bool) [int]
    if write
        var items = start
        items[0] = 50
        return items
    return List([0])

fn main()
    let items = List([10, 20])
    let looped = in_loop(items, 3)
    let branched = in_branch(items, true)
    println(f"looped {looped} branched {branched[0]} items {items[0]} {items[1]}")
"#,
        "looped 33 branched 50 items 10 20",
    );
}

/// A complete binary tree built bottom-up: each node's constructor stores the
/// two subtrees it is handed, and each subtree is wrapped in an optional the
/// moment it is built.
const TREES: &str = r#"
use system.io

class Node
    var left Node?
    var right Node?

    fn init(left Node?, right Node?)
        self.left = left
        self.right = right

fn bottom_up(depth int) Node
    if depth == 0
        return Node(None, None)
    Node(bottom_up(depth - 1), bottom_up(depth - 1))

fn check(node Node) int
    match node.left
        Some(l)
            match node.right
                Some(r): return 1 + check(l) + check(r)
                None: return 1
        None: return 1

println(f"{check(bottom_up(4))}")
"#;

/// The optional around a subtree takes the subtree's reference over, and the
/// constructor takes the optional's: building a node retains nothing, and each
/// node is released once, by whatever holds it.
#[test]
fn test_a_tree_built_bottom_up_retains_nothing_per_node() {
    assert_runs_with_output(TREES, "31");
    assert_eq!(
        super::rc_stats::rc_stats(TREES, &[]),
        "MIRI_RC_STATS: dup=0 drop=32 drop_free=32 cow_copy=0 realloc=0"
    );
}

#[test]
fn test_a_tree_built_bottom_up_is_freed_cleanly() {
    assert_heap_guard_output(TREES, "31");
}

/// A class storing the list its constructor is handed, and writing it.
const BAG: &str = r#"
use system.io
use system.collections.list

class Bag
    var items [int]

    fn init(items [int])
        self.items = items

    fn add(x int)
        self.items.push(x)
"#;

/// The caller keeps using the list it handed the constructor: the bag's write
/// copies, and neither side sees the other's push.
#[test]
fn test_a_list_the_caller_keeps_after_construction_is_unchanged_by_the_object() {
    assert_heap_guard_output(
        &format!(
            r#"{BAG}
fn main()
    var mine = List([1, 2])
    let bag = Bag(mine)
    bag.add(3)
    mine.push(9)
    println(f"{{mine.length()}} {{bag.items.length()}} {{mine[2]}} {{bag.items[2]}}")
"#
        ),
        "3 3 9 3",
    );
}

/// A list built for the constructor and handed over as it dies is the bag's
/// alone, so writing it through the bag copies nothing.
#[test]
fn test_a_list_handed_to_a_constructor_as_it_dies_is_written_in_place() {
    let code = format!(
        r#"{BAG}
let bag = Bag(List([1, 2]))
bag.add(3)
println(f"{{bag.items.length()}}")
"#
    );
    assert_runs_with_output(&code, "3");
    let line = super::rc_stats::rc_stats(&code, &[]);
    assert!(line.contains("cow_copy=0"), "{line}");
}

#[test]
fn test_a_subclass_constructor_handing_its_parameters_to_the_base_keeps_them_intact() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Base
    public name String
    public items [int]

    fn init(name String, items [int])
        self.name = name
        self.items = items

class Child extends Base
    public extra [int]

    fn init(name String, items [int], extra [int])
        super.init(name, items)
        self.extra = extra

fn main()
    var kept = List([1, 2])
    let c = Child("a" + "b", kept, List([7]))
    kept.push(3)
    c.items.push(4)
    println(f"{c.name} {kept.length()} {c.items.length()} {c.items[2]} {c.extra[0]}")
"#,
        "ab 3 3 4 7",
    );
}

#[test]
fn test_a_generic_class_constructor_stores_each_instantiation_intact() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Holder<T>
    public value T

    fn init(value T)
        self.value = value

fn main()
    let kept = List([1, 2, 3])
    let lists = Holder<[int]>(kept)
    let text = Holder<String>("x" + "y")
    let count = Holder<int>(5)
    println(f"{lists.value.length()} {kept.length()} {text.value} {count.value}")
"#,
        "3 3 xy 5",
    );
}

/// A class with a drop hook stores what its constructor is handed; a value
/// whose own release runs a hook is stored too, and every hook runs once, in
/// the order the values are released.
#[test]
fn test_constructors_of_classes_with_drop_hooks_run_each_hook_once() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Res
    var label String

    fn init(label String)
        self.label = label

    fn drop()
        println(f"drop {self.label}")

class Owner
    var res Res

    fn init(res Res)
        self.res = res

fn main()
    let a = Res("a" + "1")
    let o = Owner(Res("b" + "2"))
    println(f"{a.label} {o.res.label}")
"#,
        "a1 b2\ndrop b2\ndrop a1",
    );
}

/// A method a vtable slot points at is called through the slot as well as by
/// name, lending its argument on both paths, so it keeps borrowing it.
#[test]
fn test_a_method_called_through_a_trait_keeps_the_caller_s_list_intact() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

trait Sink
    fn put(items [int])
    fn size() int

class ListSink implements Sink
    var items [int]

    fn init()
        self.items = List<int>()

    public fn put(items [int])
        self.items = items

    public fn size() int: self.items.length()

fn fill(s Sink, items [int]): s.put(items)

fn main()
    let kept = List([1, 2])
    let alias = kept
    let sink = ListSink()
    fill(sink, kept)
    alias.push(3)
    let direct = List([4])
    let other = direct
    sink.put(direct)
    other.push(5)
    println(f"{sink.size()} {alias.length()} {other.length()}")
"#,
        "1 3 2",
    );
}

/// A method called only by name takes over what it stores: an argument an
/// alias still holds is copied by the alias's write, one handed over as it
/// dies is written in place.
#[test]
fn test_a_method_storing_its_argument_takes_a_dying_one_over() {
    let code = r#"
use system.io
use system.collections.list

class Box
    var items [int]

    fn init()
        self.items = List<int>()

    fn replace(items [int])
        self.items = items

let b = Box()
let kept = List([1])
let alias = kept
b.replace(kept)
alias.push(2)
b.replace(List([5, 6, 7]))
b.items.push(8)
println(f"{b.items.length()} {alias.length()}")
"#;
    assert_heap_guard_output(code, "4 2");
    let line = super::rc_stats::rc_stats(code, &[]);
    assert!(line.contains("cow_copy=1"), "{line}");
}
