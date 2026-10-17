// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An optional whose payload is a reference-counted pointer is that pointer,
//! null when absent: building `Some(r)` allocates nothing, and a nested
//! optional boxes only its outer layers, so `Some(None)` stays distinct from
//! `None`.

use super::utils::*;

#[test]
fn test_a_binary_tree_of_optional_children_allocates_once_per_node() {
    assert_allocation_count(
        r#"
class Node
    var left Node?
    var right Node?

    fn init(left Node?, right Node?)
        self.left = left
        self.right = right

fn bottom_up(depth int) Node
    if depth == 0
        return Node(None, None)
    Node(Some(bottom_up(depth - 1)), Some(bottom_up(depth - 1)))

fn count(node Node) int
    var total = 1
    match node.left
        Some(l): total = total + count(l)
        None: total = total + 0
    match node.right
        Some(r): total = total + count(r)
        None: total = total + 0
    total

fn main()
    let tree = bottom_up(3)
    let _nodes = count(tree)
"#,
        15,
    );
}

#[test]
fn test_a_binary_tree_of_optional_children_is_walked_and_released_whole() {
    assert_heap_guard_output(
        r#"
use system.io

class Node
    var left Node?
    var right Node?
    var value int

fn build(depth int, value int) Node
    if depth == 0
        return Node(left: None, right: None, value: value)
    Node(left: Some(build(depth - 1, value * 2)), right: Some(build(depth - 1, value * 2 + 1)), value: value)

fn total(n Node?) int
    match n
        Some(x): x.value + total(x.left) + total(x.right)
        None: 0

fn main()
    let tree = build(3, 1)
    println(f"{total(Some(tree))}")
"#,
        "120",
    );
}

#[test]
fn test_every_reference_counted_payload_kind_is_told_apart_from_none() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list
use system.collections.map
use system.collections.set

enum Color
    Red
    Green
    Custom(int)

struct Pair
    a int
    b String

class Animal
    var name String
    fn speak() String
        "..."

class Dog extends Animal
    var breed String
    fn speak() String
        "woof"

trait Shape
    fn area() int

class Square implements Shape
    var side int
    fn area() int
        self.side * self.side

fn describe_str(o String?) String
    match o
        Some(s): f"some '{s}'"
        None: "none"

fn main()
    let empty String? = Some("")
    println(describe_str(empty))
    let built String? = Some("ab" + "cd")
    println(describe_str(built))
    let nothing String? = None
    println(describe_str(nothing))

    let el List<int>? = Some(List<int>())
    match el
        Some(l): println(f"list {l.length()}")
        None: println("list none")
    let nl List<int>? = Some(List([1, 2, 3]))
    match nl
        Some(l): println(f"list {l.length()}")
        None: println("list none")

    let em Map<String, int>? = Some(Map<String, int>())
    match em
        Some(m): println(f"map {m.length()}")
        None: println("map none")
    var mm = Map<String, int>()
    mm["k"] = 1
    let nm Map<String, int>? = Some(mm)
    match nm
        Some(m): println(f"map {m.length()}")
        None: println("map none")

    let es Set<int>? = Some(Set<int>())
    match es
        Some(s): println(f"set {s.length()}")
        None: println("set none")

    let arr [int; 2]? = Some([4, 5])
    match arr
        Some(a): println(f"array {a[1]}")
        None: println("array none")

    let tup Option<(int, String)> = Some((7, "seven"))
    match tup
        Some(t): println(f"tuple {t.0} {t.1}")
        None: println("tuple none")

    let c Color? = Some(Color.Red)
    match c
        Some(col): println("color some")
        None: println("color none")
    let custom Color? = Some(Color.Custom(42))
    match custom
        Some(Color.Custom(x)): println(f"custom {x}")
        Some(other): println("other color")
        None: println("color none")

    let p Pair? = Some(Pair(a: 1, b: "x" + "y"))
    match p
        Some(v): println(f"pair {v.a} {v.b}")
        None: println("pair none")

    let k = 3
    let f Option<fn(x int) int> = Some(fn(x int) int: x + k)
    match f
        Some(g): println(f"closure {g(4)}")
        None: println("closure none")

    let sh Shape? = Some(Square(side: 3))
    match sh
        Some(s): println(f"trait {s.area()}")
        None: println("trait none")

    let an Animal? = Some(Dog(name: "rex", breed: "lab"))
    match an
        Some(a): println(f"base {a.speak()} {a.name}")
        None: println("base none")
"#,
        "some ''\nsome 'abcd'\nnone\nlist 0\nlist 3\nmap 0\nmap 1\nset 0\narray 5\ntuple 7 seven\ncolor some\ncustom 42\npair 1 xy\nclosure 7\ntrait 9\nbase woof rex",
    );
}

#[test]
fn test_optional_pointers_in_locals_fields_parameters_returns_and_collections() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list
use system.collections.map
use system.collections.set

class Node
    var value int
    var next Node?

fn label(o Node?) String
    match o
        Some(n): f"node {n.value}"
        None: "none"

fn find(xs List<Node>, v int) Node?
    for n in xs
        if n.value == v
            return Some(n)
    None

fn main()
    let a = Node(value: 1, next: None)
    let b = Node(value: 2, next: Some(a))
    println(label(b.next))
    println(label(a.next))
    let held Node? = Some(b)
    println(label(held))
    var xs = List<Node>()
    xs.push(a)
    xs.push(b)
    println(label(find(xs, 2)))
    println(label(find(xs, 9)))

    var opts = List<Node?>()
    opts.push(Some(a))
    opts.push(None)
    println(f"{label(opts[0])} {label(opts[1])} {opts.length()}")

    var m = Map<String, Node?>()
    m["x"] = Some(b)
    m["y"] = None
    let mx = label(m["x"])
    let my = label(m["y"])
    println(f"{mx} {my}")

    var s = Set<String?>()
    s.add(Some("p" + "q"))
    s.add(Some("pq"))
    s.add(None)
    s.add(None)
    let probe String? = Some("pq")
    let has_pq = s.contains(probe)
    let has_none = s.contains(None)
    println(f"set {s.length()} {has_pq} {has_none}")

    var keyed = Map<String?, int>()
    keyed[Some("k" + "1")] = 1
    keyed[Some("k1")] = 2
    keyed[None] = 3
    let k1 String? = Some("k1")
    let v1 = keyed[k1]
    let vn = keyed[None]
    println(f"keys {keyed.length()} {v1} {vn}")
"#,
        "node 1\nnone\nnode 2\nnode 2\nnone\nnode 1 none 2\nnode 2 none\nset 2 true true\nkeys 2 2 3",
    );
}

#[test]
fn test_coalescing_comparing_and_rendering_an_optional_pointer() {
    assert_runs_with_output(
        r#"
use system.io

class Node
    var value int
    var next Node?

fn main()
    let a = Node(value: 1, next: None)
    let b = Node(value: 2, next: Some(a))
    let picked = b.next ?? Node(value: 9, next: None)
    println(f"coalesce {picked.value}")
    let fallback = a.next ?? Node(value: 9, next: None)
    println(f"coalesce {fallback.value}")
    println(f"is none {a.next == None} {b.next == None}")
    let s String? = Some("hi")
    let t String? = Some("h" + "i")
    let n String? = None
    println(f"eq {s == t} {s == n} {n == None}")
    println(f"render {s} {n}")
"#,
        "coalesce 1\ncoalesce 9\nis none true false\neq true false true\nrender Some(hi) None",
    );
}

#[test]
fn test_reassigning_an_optional_class_runs_each_drop_hook_once() {
    assert_runs_with_output(
        r#"
use system.io

class Res
    var id int
    fn drop()
        println(f"drop {self.id}")

class Holder
    var r Res?

fn main()
    var held Res? = Some(Res(id: 1))
    held = Some(Res(id: 2))
    held = None
    println("after held")
    var h = Holder(r: Some(Res(id: 3)))
    h.r = Some(Res(id: 4))
    h.r = None
    println("after field")
    h.r = Some(Res(id: 5))
    println("end")
"#,
        "drop 1\ndrop 2\nafter held\ndrop 3\ndrop 4\nafter field\nend\ndrop 5",
    );
}

#[test]
fn test_none_some_none_and_some_some_stay_distinct_in_match_equality_set_and_map() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set
use system.collections.map

class Node
    var value int

fn show(o Option<Node?>) String
    match o
        Some(inner): match inner
            Some(n): f"some some {n.value}"
            None: "some none"
        None: "none"

fn main()
    let none Option<Node?> = None
    let inner_none Node? = None
    let some_none Option<Node?> = Some(inner_none)
    let n = Node(value: 7)
    let full Option<Node?> = Some(Some(n))
    println(show(none))
    println(show(some_none))
    println(show(full))
    println(f"{none == some_none} {some_none == some_none} {none == None}")
    let x Option<String?> = None
    let empty String? = None
    let y Option<String?> = Some(empty)
    let z Option<String?> = Some(Some("z"))
    var s = Set<Option<String?>>()
    s.add(x)
    s.add(y)
    s.add(z)
    s.add(y)
    println(f"set {s.length()} {s.contains(x)} {s.contains(y)}")
    var m = Map<Option<String?>, int>()
    m[x] = 1
    m[y] = 2
    m[z] = 3
    println(f"map {m.length()} {m[x]} {m[y]} {m[z]}")
"#,
        "none\nsome none\nsome some 7\nfalse true true\nset 3 true true\nmap 3 1 2 3",
    );
}

#[test]
fn test_generic_bodies_hand_back_optional_pointers_per_instantiation() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list
use system.collections.map

class Node
    var value int

class Box<T>
    var v T?

fn wrap<T>(x T) T?
    Some(x)

fn main()
    let bn = Box<Node>(v: Some(Node(value: 1)))
    match bn.v
        Some(n): println(f"box node {n.value}")
        None: println("box none")
    let bi = Box<int>(v: Some(5))
    match bi.v
        Some(i): println(f"box int {i}")
        None: println("box none")
    match wrap(Node(value: 2))
        Some(n): println(f"wrap node {n.value}")
        None: println("wrap none")
    match wrap(3)
        Some(i): println(f"wrap int {i}")
        None: println("wrap none")
    let on Node? = Some(Node(value: 4))
    match wrap(on)
        Some(inner): match inner
            Some(n): println(f"wrap opt {n.value}")
            None: println("wrap opt none")
        None: println("wrap none")
    var xs = List<Node>()
    xs.push(Node(value: 10))
    xs.push(Node(value: 11))
    xs.push(Node(value: 12))
    match xs.first()
        Some(n): println(f"first {n.value}")
        None: println("first none")
    match xs.last()
        Some(n): println(f"last {n.value}")
        None: println("last none")
    match xs.pop()
        Some(n): println(f"pop {n.value}")
        None: println("pop none")
    match xs.remove_at(0)
        Some(n): println(f"remove {n.value}")
        None: println("remove none")
    var m = Map<String, Node>()
    m["a"] = Node(value: 20)
    match m.get("a")
        Some(n): println(f"get {n.value}")
        None: println("get none")
    match m.get("b")
        Some(n): println(f"get {n.value}")
        None: println("get none")
    var ys = List<Node?>()
    ys.push(None)
    ys.push(Some(Node(value: 30)))
    match ys.pop()
        Some(inner): match inner
            Some(n): println(f"pop opt {n.value}")
            None: println("pop opt none")
        None: println("pop none")
    match ys.pop()
        Some(inner): match inner
            Some(n): println(f"pop opt {n.value}")
            None: println("pop opt none")
        None: println("pop none")
    match ys.pop()
        Some(inner): println("pop more")
        None: println("pop empty")
"#,
        "box node 1\nbox int 5\nwrap node 2\nwrap int 3\nwrap opt 4\nfirst 10\nlast 12\npop 12\nremove 10\nget 20\nget none\npop opt 30\npop opt none\npop empty",
    );
}

#[test]
fn test_sets_and_maps_of_optional_pointers_match_by_their_values() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.set
use system.collections.map

class Point
    x int
    y int

    fn init(x int, y int)
        self.x = x
        self.y = y

    public fn equals(other Point) bool
        return self.x == other.x and self.y == other.y
    public fn hash() int
        return hash_combine(self.x.hash(), self.y.hash())

class Node
    var value int

fn main()
    var points = Set<Point?>()
    points.add(Some(Point(1, 2)))
    points.add(Some(Point(1, 2)))
    points.add(None)
    let probe Point? = Some(Point(1, 2))
    let has = points.contains(probe)
    let has_none = points.contains(None)
    println(f"points {points.length()} {has} {has_none}")

    let a = Node(value: 1)
    var nodes = Set<Node?>()
    nodes.add(Some(a))
    nodes.add(Some(a))
    nodes.add(Some(Node(value: 1)))
    nodes.add(None)
    println(f"nodes {nodes.length()}")

    var by_node = Map<Node?, String>()
    by_node[Some(a)] = "a"
    by_node[None] = "none"
    let held Node? = Some(a)
    let va = by_node[held]
    let vn = by_node[None]
    println(f"by node {by_node.length()} {va} {vn}")
"#,
        "points 2 true true\nnodes 3\nby node 2 a none",
    );
}

#[test]
fn test_stacks_and_queues_hand_back_class_elements_as_optional_pointers() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.stack
use system.collections.queue

class Node
    var value int

fn main()
    var st = Stack<Node>()
    st.push(Node(value: 5))
    st.push(Node(value: 6))
    match st.peek()
        Some(n): println(f"peek {n.value}")
        None: println("peek none")
    match st.pop()
        Some(n): println(f"pop {n.value}")
        None: println("pop none")
    var q = Queue<Node>()
    q.enqueue(Node(value: 7))
    match q.peek()
        Some(n): println(f"qpeek {n.value}")
        None: println("qpeek none")
    match q.dequeue()
        Some(n): println(f"dequeue {n.value}")
        None: println("dequeue none")
    match q.dequeue()
        Some(n): println(f"dequeue {n.value}")
        None: println("dequeue none")
"#,
        "peek 6\npop 6\nqpeek 7\ndequeue 7\ndequeue none",
    );
}
