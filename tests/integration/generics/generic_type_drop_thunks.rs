// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic struct and a generic enum are released through the same
//! per-instantiation drop thunk a generic class is, so the concrete field a
//! given instantiation stores is the one the thunk decrements.
//!
//! Each of these programs is checked under the heap guard: a missing thunk
//! shows up as a link failure when the element is reached through a collection
//! and as a silently leaked payload when it is not, and only running the guard
//! separates a thunk that exists from one that releases the right field.

use super::utils::*;

const HOLDER: &str = r#"
use system.collections.list

enum Holder<T>
    One(T)
    Two(T, T)

    fn size() int
        match self
            Holder.One(_): 1
            Holder.Two(_, _): 2
"#;

fn with_holder(main: &str) -> String {
    format!("{HOLDER}\n{main}")
}

#[test]
fn a_generic_struct_with_a_collection_field_links_and_reads_it_back() {
    assert_heap_guard_output(
        r#"
use system.collections.list

struct Bag<T>
    items List<T>

fn main()
    var b = Bag<String>(items: List<String>())
    b.items.push("a" + "b")
    b.items[0] = "e" + "f"
    println(b.items[0])
"#,
        "ef",
    );
}

#[test]
fn a_generic_struct_releases_the_managed_field_of_its_instantiation() {
    assert_heap_guard_output(
        r#"
struct Wrapper<T>
    value T

fn main()
    let s = Wrapper<String>(value: "h" + "i")
    println(s.value)
"#,
        "hi",
    );
}

#[test]
fn a_generic_struct_at_a_scalar_argument_keeps_its_field_intact() {
    assert_heap_guard_output(
        r#"
struct Wrapper<T>
    value T

fn main()
    let w = Wrapper<int>(value: 42)
    println(f"{w.value}")
"#,
        "42",
    );
}

#[test]
fn a_generic_struct_held_in_a_list_releases_its_elements() {
    assert_heap_guard_output(
        r#"
use system.collections.list

struct Wrapper<T>
    value T

fn main()
    var xs = List<Wrapper<String>>()
    xs.push(Wrapper<String>(value: "o" + "n"))
    xs.push(Wrapper<String>(value: "t" + "w"))
    println(f"{xs.length()} {xs[0].value}")
"#,
        "2 on",
    );
}

#[test]
fn a_generic_enum_in_a_list_links_and_answers_its_method() {
    assert_heap_guard_output(
        &with_holder(
            r#"
fn main()
    let a Holder<int> = Holder.Two(1, 2)
    var out = List<Holder<int>>()
    out.push(a)
    println(f"{out.length()} {out[0].size()}")
"#,
        ),
        "1 2",
    );
}

#[test]
fn a_generic_enum_with_a_managed_payload_in_a_list_releases_it() {
    assert_heap_guard_output(
        &with_holder(
            r#"
fn main()
    let a Holder<String> = Holder.One("h" + "i")
    var out = List<Holder<String>>()
    out.push(a)
    println(f"{out.length()} {out[0].size()}")
"#,
        ),
        "1 1",
    );
}

#[test]
fn a_generic_enum_in_a_set_releases_its_elements() {
    assert_heap_guard_output(
        &with_holder(
            r#"
use system.collections.set

fn main()
    var s = Set<Holder<int>>()
    s.add(Holder.One(3))
    println(f"{s.length()}")
"#,
        ),
        "1",
    );
}

#[test]
fn a_generic_enum_as_a_map_value_releases_its_entries() {
    assert_heap_guard_output(
        &with_holder(
            r#"
use system.collections.map

fn main()
    var m = Map<int, Holder<String>>()
    m.set(1, Holder.One("h" + "i"))
    let got = m[1]
    println(f"{m.length()} {got.size()}")
"#,
        ),
        "1 1",
    );
}

#[test]
fn a_generic_enum_in_an_array_releases_its_elements() {
    assert_heap_guard_output(
        &with_holder(
            r#"
use system.collections.array

fn main()
    let xs Array<Holder<String>, 2> = [Holder.One("h" + "i"), Holder.Two("a" + "b", "c" + "d")]
    println(f"{xs[0].size()} {xs[1].size()}")
"#,
        ),
        "1 2",
    );
}

#[test]
fn two_instantiations_of_one_generic_enum_each_release_their_own_payload() {
    assert_heap_guard_output(
        &with_holder(
            r#"
fn main()
    var ints = List<Holder<int>>()
    ints.push(Holder.Two(1, 2))
    var texts = List<Holder<String>>()
    texts.push(Holder.One("h" + "i"))
    println(f"{ints[0].size()} {texts[0].size()}")
"#,
        ),
        "2 1",
    );
}

#[test]
fn two_instantiations_of_one_generic_struct_each_release_their_own_field() {
    assert_heap_guard_output(
        r#"
struct Wrapper<T>
    value T

fn main()
    let w = Wrapper<int>(value: 42)
    let s = Wrapper<String>(value: "h" + "i")
    println(f"{w.value} {s.value}")
"#,
        "42 hi",
    );
}
