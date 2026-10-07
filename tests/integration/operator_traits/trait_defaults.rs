// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An operator, a container's element question and the hash requirement all
//! reach the body a written call reaches — including a default a trait
//! supplies to a class that writes no method of its own.

use super::utils::*;

/// `Keyed` answers `equals` and `hash` by a key the class supplies, so two
/// instances with different fields but the same key are equal.
const KEYED: &str = r#"
use system.io
use system.collections.set
use system.collections.map
use system.collections.list

trait Keyed extends Equatable, Hashable
    fn key() int

    fn equals(other Self) bool
        return self.key() == other.key()

    fn hash() int
        return self.key()

class P implements Keyed
    v int
    label String

    fn init(v int, label String)
        self.v = v
        self.label = label

    fn key() int
        return self.v % 2
"#;

fn keyed(main: &str) -> String {
    format!("{KEYED}\n{main}")
}

#[test]
fn equality_operators_call_a_trait_default_equals() {
    assert_heap_guard_output(
        &keyed(
            r#"
fn main()
    let a = P(1, "a")
    let b = P(3, "b")
    let c = P(2, "c")
    println(f"{a == b} {a != b} {a == c} {a != c}")
"#,
        ),
        "true false false true",
    );
}

#[test]
fn list_search_matches_elements_by_a_trait_default_equals() {
    assert_heap_guard_output(
        &keyed(
            r#"
fn main()
    let items = List([P(1, "a"), P(2, "b")])
    println(f"{items.contains(P(5, 'x'))} {items.index_of(P(4, 'y'))}")
    let odd = [P(1, "a")]
    println(f"{odd.contains(P(3, 'z'))} {odd.contains(P(2, 'z'))}")
"#,
        ),
        "true Some(1)\ntrue false",
    );
}

#[test]
fn set_and_map_identify_elements_by_trait_default_equals_and_hash() {
    assert_heap_guard_output(
        &keyed(
            r#"
fn main()
    let s = Set<P>()
    s.add(P(1, "a"))
    s.add(P(3, "b"))
    s.add(P(2, "c"))
    println(f"{s.length()} {s.contains(P(5, 'd'))}")
    let m = Map<P, int>()
    m.set(P(1, "a"), 10)
    m.set(P(3, "b"), 20)
    println(f"{m.length()} {m[P(7, 'e')]}")
"#,
        ),
        "2 true\n1 20",
    );
}

#[test]
fn a_struct_holding_the_class_compares_it_by_the_trait_default() {
    assert_heap_guard_output(
        &keyed(
            r#"
struct W
    p P

fn main()
    println(f"{W(p: P(1, 'a')) == W(p: P(7, 'b'))} {W(p: P(1, 'a')) == W(p: P(2, 'a'))}")
    let s = Set<W>()
    s.add(W(p: P(1, "a")))
    s.add(W(p: P(9, "b")))
    println(f"{s.length()}")
"#,
        ),
        "true false\n1",
    );
}

#[test]
fn a_subclass_inherits_the_trait_default_its_base_implements() {
    assert_heap_guard_output(
        &keyed(
            r#"
class Q extends P
    fn init(v int)
        super.init(v, "q")

fn main()
    println(f"{Q(1) == Q(3)} {Q(1) == Q(2)}")
    println(f"{List([Q(2)]).contains(Q(4))}")
"#,
        ),
        "true false\ntrue",
    );
}

#[test]
fn a_class_equals_wins_over_the_trait_default() {
    assert_heap_guard_output(
        r#"
use system.io

trait Loose extends Equatable
    fn equals(other Self) bool
        return true

class Exact implements Loose
    v int

    fn init(v int)
        self.v = v

    fn equals(other Exact) bool
        return self.v == other.v

fn main()
    println(f"{Exact(1) == Exact(2)} {Exact(1) == Exact(1)}")
"#,
        "false true",
    );
}

#[test]
fn ordering_operators_call_a_trait_default_compare() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

trait Ranked extends Comparable
    fn rank() int

    fn compare(other Self) int
        return self.rank() - other.rank()

class R implements Ranked
    v int

    fn init(v int)
        self.v = v

    fn rank() int
        return self.v

fn main()
    println(f"{R(1) < R(3)} {R(5) >= R(3)} {R(2) > R(2)}")
    var items = List([R(3), R(1), R(2)])
    items.sort()
    println(f"{items[0].v}{items[1].v}{items[2].v}")
"#,
        "true true false\n123",
    );
}

#[test]
fn a_trait_default_equals_without_hash_is_refused_as_a_set_element() {
    assert_compiler_error(
        r#"
use system.collections.set

trait Loose extends Equatable
    fn equals(other Self) bool
        return true

class L implements Loose
    v int

    fn init(v int)
        self.v = v

fn main()
    let s = Set<L>()
    s.add(L(1))
"#,
        "'L' defines its own 'equals' but no 'hash' consistent with it",
    );
}
