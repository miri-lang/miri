// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Writing an element through a collection field: `self.items[i] = x`.
//!
//! The write releases the element it replaces, which needs the element type of
//! the field. Inside a generic class the field is declared as `List<T>`, so the
//! element is only known to be managed once `T` is read at the instantiation's
//! type argument.
//!
//! The values here are built at runtime (`"a" + "b"`) because a string literal
//! is not reference counted, which would hide a missed release.

use super::super::utils::*;

#[test]
fn test_generic_class_index_write_releases_the_replaced_string() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Bag<T>
    var items List<T>

    fn put_at(i int, x T)
        self.items[i] = x

fn main()
    var b = Bag<String>(items: List<String>())
    b.items.push("a" + "b")
    b.items.push("c" + "d")
    b.put_at(0, "e" + "f")
    b.put_at(0, "g" + "h")
    for x in b.items
        println(f"{x}")
"#,
        "gh\ncd",
    );
}

#[test]
fn test_class_index_write_releases_the_replaced_string() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Bag
    var items List<String>

    fn put_at(i int, x String)
        self.items[i] = x

fn main()
    var b = Bag(items: List<String>())
    b.items.push("a" + "b")
    b.put_at(0, "e" + "f")
    for x in b.items
        println(f"{x}")
"#,
        "ef",
    );
}

#[test]
fn test_generic_class_set_releases_the_replaced_string() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Bag<T>
    var items List<T>

    fn put_at(i int, x T)
        self.items.set(i, x)

fn main()
    var b = Bag<String>(items: List<String>())
    b.items.push("a" + "b")
    b.put_at(0, "e" + "f")
    for x in b.items
        println(f"{x}")
"#,
        "ef",
    );
}

#[test]
fn test_generic_class_index_write_releases_a_replaced_class_instance() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Tag
    var name String

class Bag<T>
    var items List<T>

    fn put_at(i int, x T)
        self.items[i] = x

fn main()
    var b = Bag<Tag>(items: List<Tag>())
    b.items.push(Tag(name: "a" + "b"))
    b.put_at(0, Tag(name: "e" + "f"))
    b.put_at(0, Tag(name: "g" + "h"))
    for x in b.items
        println(x.name)
"#,
        "gh",
    );
}

#[test]
fn test_generic_class_index_write_releases_a_replaced_list_in_a_loop() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Bag<T>
    var items List<T>

    fn put_at(i int, x T)
        self.items[i] = x

fn main()
    var b = Bag<List<String>>(items: List<List<String>>())
    var first = List<String>()
    first.push("a" + "b")
    b.items.push(first)
    for i in 0..50
        var next = List<String>()
        next.push(f"{i}")
        b.put_at(0, next)
    println(b.items[0][0])
"#,
        "49",
    );
}

#[test]
fn test_generic_class_array_field_index_write_releases_the_replaced_string() {
    assert_heap_guard_output(
        r#"
class Bag<T>
    var items [T; 2]

    fn put_at(i int, x T)
        self.items[i] = x

fn main()
    var b = Bag<String>(items: ["a" + "b", "c" + "d"])
    b.put_at(1, "e" + "f")
    println(f"{b.items[0]} {b.items[1]}")
"#,
        "ab ef",
    );
}

#[test]
fn test_generic_class_index_write_keeps_an_element_read_before_it_alive() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Bag<T>
    var items List<T>

    fn swap_in(i int, x T) T
        let old = self.items[i]
        self.items[i] = x
        return old

fn main()
    var b = Bag<String>(items: List<String>())
    b.items.push("a" + "b")
    let old = b.swap_in(0, "e" + "f")
    println(f"{old} {b.items[0]}")
"#,
        "ab ef",
    );
}

#[test]
fn test_index_write_through_a_nested_generic_field_releases_the_replaced_string() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Inner<T>
    var items List<T>

class Outer<T>
    var inner Inner<T>

    fn put_at(i int, x T)
        self.inner.items[i] = x

fn main()
    var o = Outer<String>(inner: Inner<String>(items: List<String>()))
    o.inner.items.push("a" + "b")
    o.put_at(0, "e" + "f")
    println(o.inner.items[0])
"#,
        "ef",
    );
}

#[test]
fn test_index_write_reads_the_field_at_its_own_type_parameter() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Pair<K, V>
    var keys List<K>
    var vals List<V>

    fn put(i int, k K, v V)
        self.keys[i] = k
        self.vals[i] = v

fn main()
    var p = Pair<String, int>(keys: List<String>(), vals: List<int>())
    p.keys.push("a" + "b")
    p.vals.push(1)
    p.put(0, "e" + "f", 42)
    println(f"{p.keys[0]} {p.vals[0]}")
"#,
        "ef 42",
    );
}

#[test]
fn test_generic_class_index_write_releases_a_replaced_optional() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Bag<T>
    var items List<T>

    fn put_at(i int, x T)
        self.items[i] = x

fn main()
    var b = Bag<String?>(items: List<String?>())
    b.items.push(Some("a" + "b"))
    b.put_at(0, Some("e" + "f"))
    b.put_at(0, None)
    b.put_at(0, Some("g" + "h"))
    println(b.items[0] ?? "none")
"#,
        "gh",
    );
}

#[test]
fn test_inherited_generic_field_index_write_releases_the_replaced_string() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Base<T>
    var items List<T>

class Derived<T> extends Base<T>
    var tag int

    fn put_at(i int, x T)
        self.items[i] = x

fn main()
    var d = Derived<String>(items: List<String>(), tag: 1)
    d.items.push("a" + "b")
    d.put_at(0, "e" + "f")
    println(d.items[0])
"#,
        "ef",
    );
}

#[test]
fn test_value_generic_class_index_write_releases_the_replaced_string() {
    assert_heap_guard_output(
        r#"
class Buf<T, Size>
    var items Array<T, Size>

    fn put_at(i int, x T)
        self.items[i] = x

fn main()
    var b = Buf<String, 2>(items: ["a" + "b", "c" + "d"])
    b.put_at(0, "e" + "f")
    println(f"{b.items[0]} {b.items[1]}")
"#,
        "ef cd",
    );
}

#[test]
fn test_value_generic_class_index_write_at_a_float_stores_the_value() {
    assert_heap_guard_output(
        r#"
class Buf<T, Size>
    var items Array<T, Size>

    fn put_at(i int, x T)
        self.items[i] = x

fn main()
    var f = Buf<float, 2>(items: [1.5, 2.5])
    f.put_at(1, 9.75)
    println(f"{f.items[0]} {f.items[1]}")
"#,
        "1.5 9.75",
    );
}

#[test]
fn test_generic_class_index_write_at_a_scalar_stores_the_value() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Bag<T>
    var items List<T>

    fn put_at(i int, x T)
        self.items[i] = x

fn main()
    var n = Bag<int>(items: List<int>())
    n.items.push(1)
    n.items.push(2)
    n.put_at(1, 70000000000)
    var f = Bag<float>(items: List<float>())
    f.items.push(1.5)
    f.items.push(2.5)
    f.put_at(0, 3.25)
    var t = Bag<bool>(items: List<bool>())
    t.items.push(false)
    t.put_at(0, true)
    println(f"{n.items[0]} {n.items[1]} {f.items[0]} {f.items[1]} {t.items[0]}")
"#,
        "1 70000000000 3.25 2.5 true",
    );
}

/// Writing through two indexes at once (`self.rows[r][c] = x`) reaches the
/// element through the inner collection, and the receiver of that second index
/// is a retained copy of it. The copy exists only for the write, so it has to
/// give its reference back — while a receiver that was already a binding must
/// not be released, since the program goes on using it.
#[test]
fn a_nested_index_write_through_a_field_releases_the_inner_collection() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Grid
    var rows List<List<String>>

    fn put(r int, c int, x String)
        self.rows[r][c] = x

fn main()
    var row = List<String>()
    row.push("a" + "b")
    var g = Grid(rows: List<List<String>>())
    g.rows.push(row)
    g.put(0, 0, "e" + "f")
    println(g.rows[0][0])
"#,
        "ef",
    );
}

#[test]
fn a_nested_index_write_through_a_generic_field_releases_the_inner_collection() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Grid<T>
    var rows List<List<T>>

    fn put(r int, c int, x T)
        self.rows[r][c] = x

fn main()
    var row = List<String>()
    row.push("a" + "b")
    var g = Grid<String>(rows: List<List<String>>())
    g.rows.push(row)
    g.put(0, 0, "e" + "f")
    println(g.rows[0][0])
"#,
        "ef",
    );
}

#[test]
fn a_three_level_index_write_releases_every_collection_it_passes_through() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Cube
    var cells List<List<List<String>>>

    fn put(a int, b int, c int, x String)
        self.cells[a][b][c] = x

fn main()
    var inner = List<String>()
    inner.push("a" + "b")
    var middle = List<List<String>>()
    middle.push(inner)
    var q = Cube(cells: List<List<List<String>>>())
    q.cells.push(middle)
    q.put(0, 0, 0, "e" + "f")
    println(q.cells[0][0][0])
"#,
        "ef",
    );
}

/// The receiver of a nested write is sometimes a plain binding rather than a
/// temp made for the write. Releasing that would take a reference the program
/// still needs, so the binding has to survive the write intact.
#[test]
fn a_nested_index_write_over_a_local_leaves_the_binding_usable() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    var row = List<String>()
    row.push("a" + "b")
    var rows = List<List<String>>()
    rows.push(row)
    rows[0][0] = "e" + "f"
    let again = rows[0][0]
    println(f"{again} {rows.length()}")
"#,
        "ef 1",
    );
}
