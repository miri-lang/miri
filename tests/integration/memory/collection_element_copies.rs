// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A collection copies an element only through the element's own `clone`.
//!
//! `clone`, `filter`, `resize` and their kin copy each element they keep
//! with its `clone()` when its type implements `Cloneable`, so every copy is
//! its own value with its own hook: every clone and every drop runs, in the
//! order asserted, under the heap guard. At a resource with no `clone` the
//! collection can call the call is refused with MER_OWN_010 where the program
//! makes it, the note naming the library line making the copy, and the
//! repair that hands each element over runs. A plain value that does not
//! implement `Cloneable` is shared as before; a plain value that does is
//! cloned per copy. A collection of a resource that is never copied
//! compiles.

use super::super::utils::*;

const CODE: &str = "MER_OWN_010";

/// A resource that implements `Cloneable`, its clone numbered ten above.
const CLONEABLE: &str = r#"use system.collections.list
use system.collections.map
use system.collections.set
use system.memory

class Res implements Cloneable
    public var id int
    fn init(id int)
        self.id = id
    fn clone() Res
        println(f"clone {self.id}")
        return Res(self.id + 10)
    fn drop(self)
        println(f"drop {self.id}")
    public fn equals(other Res) bool
        return self.id == other.id
    public fn hash() int
        return self.id
"#;

/// A resource that does not implement `Cloneable`.
const PLAIN_RESOURCE: &str = r#"use system.collections.list
use system.collections.map
use system.collections.set

class Res
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"drop {self.id}")
    public fn equals(other Res) bool
        return self.id == other.id
    public fn hash() int
        return self.id
"#;

fn cloneable(main: &str) -> String {
    format!("{CLONEABLE}{main}")
}

fn plain_resource(main: &str) -> String {
    format!("{PLAIN_RESOURCE}{main}")
}

/// Asserts `source` is refused with MER_OWN_010 at the line containing
/// `call`, the note naming the library file `copier`.
fn assert_copy_refused(source: &str, call: &str, copier: &str) {
    let diagnostic = assert_refused_at(source, CODE, call);
    let names_the_copier = (diagnostic.related.iter())
        .any(|note| note.message.contains("the copy is made at") && note.message.contains(copier));
    assert!(
        diagnostic.message.contains("copies elements of `Res`") && names_the_copier,
        "the refusal names the element and the library line: {diagnostic:#?}"
    );
    let help = diagnostic.help.as_deref().unwrap_or_default();
    assert!(
        help.contains("implement `Cloneable` on `Res`"),
        "the refusal carries the repair: {diagnostic:#?}"
    );
}

#[test]
fn cloning_a_list_of_a_cloneable_resource_clones_each_element() {
    assert_heap_guard_output(
        &cloneable(
            r#"
fn main()
    let l = List([Res(1), Res(2)])
    let c = l.clone()
    println(f"{c[0].id} {c[1].id}")
"#,
        ),
        "clone 1\nclone 2\n11 12\ndrop 11\ndrop 12\ndrop 1\ndrop 2",
    );
}

#[test]
fn cloning_an_empty_list_of_a_cloneable_resource_runs_no_hook() {
    assert_heap_guard_output(
        &cloneable(
            r#"
fn main()
    let l = List<Res>()
    let c = l.clone()
    println(f"{c.length()}")
"#,
        ),
        "0",
    );
}

#[test]
fn filtering_a_list_of_a_cloneable_resource_clones_each_element_kept() {
    assert_heap_guard_output(
        &cloneable(
            r#"
fn main()
    let l = List([Res(1), Res(2), Res(3)])
    let f = l.filter(fn(r Res) bool: r.id > 1)
    println(f"{f.length()}")
"#,
        ),
        "clone 2\nclone 3\n2\ndrop 12\ndrop 13\ndrop 1\ndrop 2\ndrop 3",
    );
}

#[test]
fn mapping_a_list_of_a_cloneable_resource_copies_nothing() {
    assert_heap_guard_output(
        &cloneable(
            r#"
fn main()
    let l = List([Res(1), Res(2)])
    let m = l.map(fn(r Res) Res: Res(r.id + 100))
    println(f"{m[0].id} {m[1].id}")
"#,
        ),
        "101 102\ndrop 101\ndrop 102\ndrop 1\ndrop 2",
    );
}

#[test]
fn resizing_a_list_of_a_cloneable_resource_clones_the_value_per_element() {
    assert_heap_guard_output(
        &cloneable(
            r#"
fn main()
    var l = List<Res>()
    l.resize(2, Res(5))
    println(f"{l.length()}")
"#,
        ),
        "clone 5\nclone 5\ndrop 5\n2\ndrop 15\ndrop 15",
    );
}

#[test]
fn resizing_a_list_of_a_cloneable_resource_to_its_length_or_shorter_clones_nothing() {
    assert_heap_guard_output(
        &cloneable(
            r#"
fn main()
    var l = List([Res(1), Res(2), Res(3)])
    l.resize(3, Res(7))
    println(f"same {l.length()}")
    l.resize(1, Res(8))
    println(f"short {l.length()}")
"#,
        ),
        "drop 7\nsame 3\ndrop 3\ndrop 2\ndrop 8\nshort 1\ndrop 1",
    );
}

#[test]
fn filtering_a_map_of_a_cloneable_resource_clones_each_value_kept() {
    assert_heap_guard_output(
        &cloneable(
            r#"
fn main()
    var m = Map<int, Res>()
    m.set(1, Res(1))
    m.set(2, Res(2))
    let f = m.filter(fn(k int, v Res) bool: k == 2)
    println(f"{f.length()}")
"#,
        ),
        "clone 2\n1\ndrop 12\ndrop 2\ndrop 1",
    );
}

#[test]
fn a_map_filter_keeping_nothing_clones_nothing() {
    assert_heap_guard_output(
        &cloneable(
            r#"
fn main()
    var m = Map<int, Res>()
    m.set(1, Res(1))
    let f = m.filter(fn(k int, v Res) bool: false)
    println(f"{f.length()}")
"#,
        ),
        "0\ndrop 1",
    );
}

#[test]
fn filtering_a_set_of_a_cloneable_resource_clones_each_element_kept() {
    assert_heap_guard_output(
        &cloneable(
            r#"
fn main()
    var s = Set<Res>()
    s.add(Res(1))
    s.add(Res(2))
    let f = s.filter(fn(r Res) bool: r.id == 2)
    println(f"{f.length()}")
"#,
        ),
        "clone 2\n1\ndrop 12\ndrop 2\ndrop 1",
    );
}

#[test]
fn cloning_a_list_of_a_resource_that_is_not_cloneable_is_refused_at_the_call() {
    assert_copy_refused(
        &plain_resource(
            r#"
fn main()
    let l = List([Res(1), Res(2)])
    let c = l.clone()
    println(f"{c.length()}")
"#,
        ),
        "let c = l.clone()",
        "list.mi:",
    );
}

#[test]
fn cloning_an_empty_list_of_a_resource_that_is_not_cloneable_is_refused_still() {
    assert_copy_refused(
        &plain_resource(
            r#"
fn main()
    let l = List<Res>()
    let c = l.clone()
    println(f"{c.length()}")
"#,
        ),
        "let c = l.clone()",
        "list.mi:",
    );
}

#[test]
fn filtering_a_list_of_a_resource_that_is_not_cloneable_is_refused_at_the_call() {
    assert_copy_refused(
        &plain_resource(
            r#"
fn main()
    let l = List([Res(1), Res(2)])
    let f = l.filter(fn(r Res) bool: r.id > 1)
    println(f"{f.length()}")
"#,
        ),
        "let f = l.filter(",
        "list.mi:",
    );
}

#[test]
fn resizing_a_list_of_a_resource_that_is_not_cloneable_is_refused_at_the_call() {
    assert_copy_refused(
        &plain_resource(
            r#"
fn main()
    var l = List<Res>()
    l.resize(2, Res(5))
    println(f"{l.length()}")
"#,
        ),
        "l.resize(2, Res(5))",
        "list.mi:",
    );
}

#[test]
fn sorting_a_copy_of_a_list_of_a_resource_that_is_not_cloneable_is_refused_at_the_call() {
    assert_copy_refused(
        &plain_resource(
            r#"
fn main()
    let l = List([Res(2), Res(1)])
    let s = l.sorted_by(fn(a Res, b Res) int: a.id - b.id)
    println(f"{s.length()}")
"#,
        ),
        "let s = l.sorted_by(",
        "list.mi:",
    );
}

#[test]
fn filtering_a_map_of_a_resource_that_is_not_cloneable_is_refused_at_the_call() {
    assert_copy_refused(
        &plain_resource(
            r#"
fn main()
    var m = Map<int, Res>()
    m.set(1, Res(1))
    let f = m.filter(fn(k int, v Res) bool: true)
    println(f"{f.length()}")
"#,
        ),
        "let f = m.filter(",
        "map.mi:",
    );
}

#[test]
fn filtering_a_map_keyed_by_a_resource_is_refused_at_the_call() {
    assert_copy_refused(
        &plain_resource(
            r#"
fn main()
    var m = Map<Res, int>()
    m.set(Res(1), 1)
    let f = m.filter(fn(k Res, v int) bool: true)
    println(f"{f.length()}")
"#,
        ),
        "let f = m.filter(",
        "map.mi:",
    );
}

#[test]
fn filtering_a_set_of_a_resource_that_is_not_cloneable_is_refused_at_the_call() {
    assert_copy_refused(
        &plain_resource(
            r#"
fn main()
    var s = Set<Res>()
    s.add(Res(1))
    let f = s.filter(fn(r Res) bool: true)
    println(f"{f.length()}")
"#,
        ),
        "let f = s.filter(",
        "set.mi:",
    );
}

#[test]
fn moving_the_elements_out_repairs_a_refused_copy() {
    assert_heap_guard_output(
        &plain_resource(
            r#"
fn main()
    var l = List([Res(1), Res(2)])
    var kept = List<Res>()
    while l.length() > 0
        match l.remove_at(0)
            Some(r):
                if r.id > 1: kept.push(r)
            None: break
    var grown = List<Res>()
    for i in 0..2: grown.push(Res(5))
    println(f"{kept.length()} {grown.length()}")
"#,
        ),
        "drop 1\n1 2\ndrop 5\ndrop 5\ndrop 2",
    );
}

#[test]
fn mapping_a_list_of_a_resource_that_is_not_cloneable_copies_nothing() {
    assert_heap_guard_output(
        &plain_resource(
            r#"
fn main()
    let l = List([Res(1), Res(2)])
    let m = l.map(fn(r Res) Res: Res(r.id + 100))
    println(f"{m.length()}")
"#,
        ),
        "2\ndrop 101\ndrop 102\ndrop 1\ndrop 2",
    );
}

#[test]
fn a_list_of_a_resource_that_is_never_copied_compiles() {
    assert_heap_guard_output(
        &plain_resource(
            r#"
fn main()
    var l = List<Res>()
    l.push(Res(1))
    let any = l.any(fn(r Res) bool: r.id == 1)
    println(f"{l.length()} {any}")
"#,
        ),
        "1 true\ndrop 1",
    );
}

#[test]
fn copying_a_collection_of_plain_values_shares_them_as_before() {
    assert_heap_guard_output(
        r#"
use system.collections.list
use system.collections.set

fn main()
    let l = List(["a", "b"])
    let c = l.clone()
    let f = l.filter(fn(s String) bool: s == "b")
    var r = List<int>()
    r.resize(3, 7)
    var s = Set<String>()
    s.add("x")
    let t = s.filter(fn(e String) bool: true)
    println(f"{c[0]} {f[0]} {r[2]} {r.length()} {t.length()}")
"#,
        "a b 7 3 1",
    );
}

#[test]
fn copying_a_collection_of_a_plain_cloneable_value_clones_each_copy() {
    assert_heap_guard_output(
        r#"
use system.collections.list
use system.memory

class Pt implements Cloneable
    public var x int
    fn init(x int)
        self.x = x
    fn clone() Pt
        println(f"clone {self.x}")
        return Pt(self.x)

fn main()
    let l = List([Pt(1), Pt(2)])
    let c = l.clone()
    let f = l.filter(fn(p Pt) bool: p.x == 2)
    var r = List<Pt>()
    r.resize(2, Pt(7))
    println(f"{c.length()} {f.length()} {r.length()}")
"#,
        "clone 1\nclone 2\nclone 2\nclone 7\nclone 7\n2 1 2",
    );
}

/// An enum with a drop hook has no `clone` a collection can call, whether
/// or not it implements `Cloneable`: a copy of one is refused.
const HOOKED_ENUM: &str = r#"use system.collections.list
use system.memory

enum Res
    Tag(int)

    fn drop(self)
        match self
            Res.Tag(n): println(f"drop {n}")
"#;

#[test]
fn cloning_a_list_of_an_enum_with_a_hook_is_refused_at_the_call() {
    let source = format!(
        "{HOOKED_ENUM}\nfn main()\n    let l = List([Res.Tag(1)])\n    let c = l.clone()\n    println(f\"{{c.length()}}\")\n"
    );
    assert_refused_at(&source, CODE, "let c = l.clone()");
}

#[test]
fn filtering_a_list_of_an_enum_with_a_hook_is_refused_at_the_call() {
    let source = format!(
        "{HOOKED_ENUM}\nfn main()\n    let l = List([Res.Tag(1)])\n    let f = l.filter(fn(r Res) bool: true)\n    println(f\"{{f.length()}}\")\n"
    );
    assert_refused_at(&source, CODE, "let f = l.filter(");
}

/// A list of lists, built by handing each list over, then cloned: the copy
/// of the outer list copies each inner list, and the copy of each inner list
/// copies each of its elements through the element's `clone`, so the copy
/// holds values of its own all the way down.
const NESTED: &str = r#"
fn main()
    var inner = List<Res>()
    inner.push(Res(1))
    var l = List<List<Res>>()
    l.push(inner)
    let c = l.clone()
    println(f"{c.length()} {c[0][0].id}")
"#;

/// Building the outer list copies nothing, so it runs at a resource of any
/// kind; only the clone copies.
const NESTED_BUILT: &str = r#"
fn main()
    var inner = List<Res>()
    inner.push(Res(1))
    var l = List<List<Res>>()
    l.push(inner)
    println(f"{l.length()} {l[0][0].id}")
"#;

#[test]
fn cloning_a_list_of_lists_of_a_cloneable_resource_clones_each_inner_element() {
    assert_heap_guard_output(&cloneable(NESTED), "clone 1\n1 11\ndrop 11\ndrop 1");
}

#[test]
fn building_a_list_of_lists_of_a_resource_that_is_not_cloneable_runs() {
    assert_heap_guard_output(&plain_resource(NESTED_BUILT), "1 1\ndrop 1");
}

/// Cloning a list of lists of a resource with no `clone` is refused at the
/// clone: copying each inner list would copy the resource.
#[test]
fn cloning_a_list_of_lists_of_a_resource_that_is_not_cloneable_is_refused_at_the_clone() {
    assert_refused_at(&plain_resource(NESTED), CODE, "let c = l.clone()");
}

/// A generic function copying a list of its own type parameter.
const DUP: &str = "\nfn dup<T>(l List<T>) List<T>\n    return l.clone()\n";

#[test]
fn a_generic_function_copying_a_list_is_refused_where_it_is_reached_at_a_resource() {
    let source = plain_resource(&format!(
        "{DUP}\nfn main()\n    let d = dup(List([Res(1)]))\n    println(f\"{{d.length()}}\")\n"
    ));
    assert_refused_at(&source, CODE, "let d = dup(");
}

#[test]
fn a_generic_function_copying_a_list_runs_at_a_plain_value_and_at_a_cloneable_resource() {
    assert_heap_guard_output(
        &cloneable(&format!(
            "{DUP}\nfn main()\n    let n = dup(List([1, 2]))\n    let d = dup(List([Res(1)]))\n    println(f\"{{n.length()}} {{d[0].id}}\")\n"
        )),
        "clone 1\ndrop 1\n2 11\ndrop 11",
    );
}

#[test]
fn a_generic_copy_reached_only_at_a_plain_value_compiles_beside_a_resource() {
    assert_heap_guard_output(
        &plain_resource(&format!(
            "{DUP}\nfn main()\n    let r = Res(1)\n    let n = dup(List([1, 2]))\n    println(f\"{{n.length()}} {{r.id}}\")\n"
        )),
        "2 1\ndrop 1",
    );
}

/// A list built from another list: the constructor copies the list as its
/// `clone` does.
const FROM_LIST: &str = r#"
fn main()
    var l = List<Res>()
    l.push(Res(1))
    let d = List(l)
    println(f"{d.length()} {d[0].id}")
"#;

#[test]
fn building_a_list_from_a_list_of_a_resource_that_is_not_cloneable_is_refused_at_the_call() {
    let diagnostic = assert_refused_at(&plain_resource(FROM_LIST), CODE, "let d = List(l)");
    let help = diagnostic.help.unwrap_or_default();
    assert!(
        help.contains("implement `Cloneable` on `Res`"),
        "the repair is the clone's: {help}"
    );
}

#[test]
fn building_a_list_from_a_list_of_a_cloneable_resource_clones_each_element() {
    assert_heap_guard_output(&cloneable(FROM_LIST), "clone 1\n1 11\ndrop 11\ndrop 1");
}

#[test]
fn building_a_list_from_a_list_of_a_plain_value_copies_it() {
    assert_heap_guard_output(
        "use system.collections.list\n\nfn main()\n    var l = List([1, 2])\n    var d = List(l)\n    d.push(3)\n    println(f\"{l.length()} {d.length()}\")\n",
        "2 3",
    );
}

/// A generic function building a list from a list of its own parameter.
const DUP_BUILT: &str = "\nfn dup<T>(l List<T>) List<T>\n    return List(l)\n";

#[test]
fn a_generic_function_building_a_list_from_a_list_is_refused_where_it_is_reached_at_a_resource() {
    let source = plain_resource(&format!(
        "{DUP_BUILT}\nfn main()\n    var l = List<Res>()\n    l.push(Res(1))\n    let d = dup(l)\n    println(f\"{{d.length()}}\")\n"
    ));
    let diagnostic = assert_refused_at(&source, CODE, "let d = dup(l)");
    assert!(
        (diagnostic.related.iter()).any(|note| note.message.contains("list.mi")),
        "a note names the library line making the copy: {diagnostic:#?}"
    );
}

#[test]
fn a_generic_function_building_a_list_from_a_list_runs_at_a_cloneable_resource_and_at_a_plain_value(
) {
    assert_heap_guard_output(
        &cloneable(&format!(
            "{DUP_BUILT}\nfn main()\n    let n = dup(List([1, 2]))\n    var l = List<Res>()\n    l.push(Res(1))\n    let d = dup(l)\n    println(f\"{{n.length()}} {{d[0].id}}\")\n"
        )),
        "clone 1\n2 11\ndrop 11\ndrop 1",
    );
}

/// A generic class declaring its own `filter`, and one inheriting it.
const ZOO: &str = r#"
class Zoo<T> implements Sequenced<T>
    var items List<T>
    fn init()
        self.items = List<T>()
    fn add(item T)
        self.items.push(item)
    fn length() int
        return self.items.length()
    fn element_at(index int) T
        return self.items[index]
    fn iterator() Iterator<T>
        return self.items.iterator()
    public fn filter(f fn(T) bool) [T]
        var result = List<T>()
        for item in self.items
            if f(item): result.push_copy(item)
        return result

class Child<T> extends Zoo<T>
    fn init()
        super.init()
"#;

#[test]
fn a_copy_in_a_class_method_an_inheriting_class_runs_is_refused_at_a_resource() {
    let source = format!(
        "use system.collections.sequenced\nuse system.ops\n{}{ZOO}\nfn main()\n    let z = Child<Res>()\n    z.add(Res(1))\n    let f = z.filter(fn(r Res) bool: true)\n    println(f\"{{f.length()}}\")\n",
        PLAIN_RESOURCE
    );
    assert_refused_at(&source, CODE, "let f = z.filter(");
}

#[test]
fn a_list_of_strings_filtered_in_a_generic_function_outlives_its_source() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn keep<T>(l List<T>, f fn(T) bool) List<T>
    return l.filter(f)

fn build() List<String>
    let source = List(["a" + "1", "bb" + "2", "ccc" + "3"])
    let kept = keep(source, fn(s String) bool: s.length() > 2)
    return kept.take(1)

fn main()
    let first = build()
    println(first[0])
"#,
        "bb2",
    );
}

#[test]
fn a_list_of_strings_filtered_through_a_trait_default_outlives_its_source() {
    assert_heap_guard_output(
        r#"
use system.collections.list
use system.collections.sequenced
use system.ops

class Bag implements Sequenced<String>
    var items [String]
    fn init(items [String])
        self.items = items
    fn length() int
        return self.items.length()
    fn element_at(index int) String
        return self.items[index]
    fn iterator() Iterator<String>
        return self.items.iterator()

fn build() List<String>
    let bag = Bag(List(["a" + "1", "bb" + "2", "ccc" + "3"]))
    let kept = bag.filter(fn(s String) bool: s.length() > 2)
    return kept.take(1)

fn main()
    let first = build()
    println(first[0])
"#,
        "bb2",
    );
}

/// The resource declared in a module of its own.
const MODULE_RESOURCE: (&str, &str) = (
    "conn.mi",
    "public class Res\n    public var id int\n    fn init(id int)\n        self.id = id\n    fn drop(self)\n        println(f\"drop {self.id}\")\n",
);

#[test]
fn a_resource_declared_in_a_module_is_refused_the_same_copy() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use system.collections.list\n",
                    "use local.conn\n",
                    "\n",
                    "fn main()\n",
                    "    var l = List<Res>()\n",
                    "    l.resize(2, Res(5))\n",
                    "    println(f\"{l.length()}\")\n",
                ),
            ),
            MODULE_RESOURCE,
        ],
        "copies elements of `Res`, a resource with no `clone` to copy it with",
    );
}

#[test]
fn a_resource_declared_in_a_module_compiles_where_it_is_never_copied() {
    assert_project_heap_guard_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.collections.list\n",
                    "use local.conn\n",
                    "\n",
                    "fn main()\n",
                    "    var l = List<Res>()\n",
                    "    l.push(Res(1))\n",
                    "    println(f\"{l.length()}\")\n",
                ),
            ),
            MODULE_RESOURCE,
        ],
        "1\ndrop 1",
    );
}

/// Whether a nested list is refused and whether its copy clones each inner
/// element agree: exactly one holds, for a `Cloneable` resource, a plain
/// one, and an abstract `Cloneable` base, which has no `clone` of its own.
#[test]
fn a_nested_list_is_either_refused_or_cloned_all_the_way_down() {
    let abstract_base = r#"use system.collections.list
use system.memory

abstract class Res implements Cloneable
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"drop {self.id}")

class Leaf extends Res
    fn init(id int)
        super.init(id)
    fn clone() Leaf
        return Leaf(self.id + 10)

fn main()
    var inner = List<Res>()
    inner.push(Leaf(1))
    var l = List<List<Res>>()
    l.push(inner)
    let c = l.clone()
    println(f"{c.length()} {c[0][0].id}")
"#;
    assert_heap_guard_output(&cloneable(NESTED), "clone 1\n1 11\ndrop 11\ndrop 1");
    assert_refused_at(&plain_resource(NESTED), CODE, "let c = l.clone()");
    assert_refused_at(abstract_base, CODE, "let c = l.clone()");
}

/// A list of an inline struct holding a counted string: the copy of the
/// list retains each string it copies, so writing either list leaves the
/// other whole and every string is released once.
#[test]
fn cloning_a_list_of_a_struct_holding_a_string_shares_each_string_soundly() {
    assert_heap_guard_output(
        r#"use system.collections.list

struct Tag
    name String
    n int

fn main()
    var l = List<Tag>()
    l.push(Tag(name: "a" + "b", n: 1))
    l.push(Tag(name: "c" + "d", n: 2))
    var c = l.clone()
    c.push(Tag(name: "e" + "f", n: 3))
    l.clear()
    println(f"{l.length()} {c.length()} {c[0].name} {c[1].name} {c[2].n}")
"#,
        "0 3 ab cd 3",
    );
}
