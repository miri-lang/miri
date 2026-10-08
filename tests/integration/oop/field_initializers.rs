// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A field declared with an initializer (`var v = 7`) starts at that value in
//! every instance whose constructor call does not set it, and the initializer
//! runs once per construction.

use super::utils::*;

#[test]
fn test_omitted_field_starts_at_its_initializer() {
    assert_runs_with_output(
        r#"
use system.io

class Crate
    var v = 7
    var label = "crate"

fn main()
    let c = Crate()
    println(f"{c.v} {c.label}")
"#,
        "7 crate",
    );
}

#[test]
fn test_constructor_argument_overrides_field_initializer() {
    assert_runs_with_output(
        r#"
use system.io

class Crate
    var v int = 7
    var label = "crate"

fn main()
    let named = Crate(label: "box")
    let positional = Crate(3)
    println(f"{named.v} {named.label} {positional.v} {positional.label}")
"#,
        "7 box 3 crate",
    );
}

#[test]
fn test_init_sees_initialized_fields() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Crate
    var v int = 7
    var items = List([1, 2])

    fn init(extra int)
        self.v = self.v + extra
        self.items.push(extra)

fn main()
    let c = Crate(extra: 3)
    println(f"{c.v} {c.items.length()}")
"#,
        "10 3",
    );
}

#[test]
fn test_managed_initializer_builds_a_fresh_value_per_instance() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Bag
    var items = List([1, 2])
    var name String = "bag"

fn main()
    var i = 0
    while i < 3
        let a = Bag()
        let b = Bag()
        a.items.push(i)
        println(f"{a.items.length()} {b.items.length()} {a.name}")
        i = i + 1
"#,
        "3 2 bag\n3 2 bag\n3 2 bag",
    );
}

#[test]
fn test_init_replacing_a_managed_initialized_field_releases_the_initial_value() {
    assert_heap_guard_output(
        r#"
use system.io

fn label() String
    return "made" + "!"

class Base
    var tag = label()

    fn init(t String)
        self.tag = t

class Child extends Base
    var extra = label()

fn main()
    let c = Child(t: "x" + "y")
    let d = Child(t: "z")
    println(f"{c.tag} {c.extra} {d.tag}")
"#,
        "xy made! z",
    );
}

#[test]
fn test_inherited_field_starts_at_the_base_initializer() {
    assert_runs_with_output(
        r#"
use system.io

class Base
    var tag = "base"
    var count int = 4

class Child extends Base
    var extra = 9

fn main()
    let c = Child()
    println(f"{c.tag} {c.count} {c.extra}")
"#,
        "base 4 9",
    );
}

#[test]
fn test_field_initializer_runs_once_per_construction_only_when_omitted() {
    assert_runs_with_output(
        r#"
use system.io

fn seed() int
    println("seeded")
    return 4

class Crate
    var v = seed()

fn main()
    let a = Crate()
    let b = Crate()
    let c = Crate(v: 1)
    println(f"{a.v} {b.v} {c.v}")
"#,
        "seeded\nseeded\n4 4 1",
    );
}

#[test]
fn test_field_initializer_must_match_the_declared_type() {
    assert_compiler_error(
        r#"
use system.io

class Crate
    var v int = "seven"

fn main()
    let c = Crate()
    println(f"{c.v}")
"#,
        "Type mismatch",
    );
}
