// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_init_body_executes() {
    assert_runs_with_output(
        r#"
use system.io

class Greeter
    var name String
    fn init(n String)
        self.name = n
        println("constructed")

fn main()
    let g = Greeter(n: "world")
    println(g.name)
    "#,
        "constructed\nworld",
    );
}

#[test]
fn test_init_with_self_field_assignment() {
    assert_runs_with_output(
        r#"
use system.io

class Dog
    var name String
    var age int
    fn init(n String, a int)
        self.name = n
        self.age = a

fn main()
    let d = Dog(n: "Rex", a: 5)
    println(f"{d.name} is {d.age} years old")
    "#,
        "Rex is 5 years old",
    );
}

#[test]
fn test_init_with_logic() {
    assert_runs_with_output(
        r#"
use system.io

class Counter
    var count int = 0
    fn init(start int)
        self.count = start * 2

fn main()
    let c = Counter(start: 3)
    println(f"{c.count}")
    "#,
        "6",
    );
}

#[test]
fn test_class_without_init_still_works() {
    assert_runs_with_output(
        r#"
use system.io

class Point
    var x int
    var y int

fn main()
    let p = Point(x: 10, y: 20)
    println(f"{p.x + p.y}")
    "#,
        "30",
    );
}

#[test]
fn test_super_init_chaining() {
    assert_runs_with_output(
        r#"
use system.io

class Animal
    var name String
    fn init(n String)
        self.name = n
        println("Animal init")

class Dog extends Animal
    var breed String
    fn init(n String, b String)
        super.init(n)
        self.breed = b
        println("Dog init")

fn main()
    let d = Dog(n: "Rex", b: "Lab")
    println(d.breed)
    "#,
        "Animal init\nDog init\nLab",
    );
}

// ── Constructor edge cases ────────────────────────────────────────────────────

#[test]
fn test_base_class_no_arg_init_called_automatically() {
    // When the base class has a no-arg `init` and the subclass provides no `init`,
    // the base constructor should still run (auto-inherited constructor).
    assert_runs_with_output(
        r#"
use system.io

class Base
    var ready bool
    fn init()
        self.ready = true

class Sub extends Base

fn main()
    let s = Sub()
    println(f"{s.ready}")
    "#,
        "true",
    );
}

#[test]
fn test_super_init_not_called_is_compiler_error() {
    // The compiler ENFORCES that super.init() must be called when the parent
    // defines a constructor — omitting it is a static error.
    assert_compiler_error(
        r#"
class Animal
    var name String
    fn init(n String)
        self.name = n

class Dog extends Animal
    var breed String
    fn init(b String)
        self.breed = b

fn main()
    let d = Dog(b: "Lab")
    "#,
        "must call super.init()",
    );
}

#[test]
fn test_init_callable_as_method() {
    // Calling `obj.init(...)` explicitly after construction is unusual but the
    // language must either allow it cleanly or give a clear compile/runtime error.
    // This test documents current behavior — adjust expected output as needed.
    assert_runs_with_output(
        r#"
use system.io

class Counter
    var count int
    fn init(n int)
        self.count = n

fn main()
    var c = Counter(n: 1)
    c.init(42)
    println(f"{c.count}")
    "#,
        "42",
    );
}

#[test]
fn test_conditional_super_init_is_compiler_error() {
    // super.init() inside an `if` branch is not guaranteed on all paths.
    // The compiler should reject this, but currently passes it without error.
    // Fix: add a reachability check in the constructor validator that ensures
    // super.init() is called on every code path before any other statement.
    assert_compiler_error(
        r#"
class Base
    var value int
    fn init(v int)
        self.value = v

class Child extends Base
    var extra int
    fn init(v int, skip bool)
        if not skip
            super.init(v)
        self.extra = v * 2
    "#,
        "must call super.init()",
    );
}

#[test]
fn test_init_multiple_super_levels() {
    // Three-level chain: Leaf → Mid → Root, all inits called.
    assert_runs_with_output(
        r#"
use system.io

class Root
    var r int
    fn init(rv int)
        self.r = rv

class Mid extends Root
    var m int
    fn init(rv int, mv int)
        super.init(rv)
        self.m = mv

class Leaf extends Mid
    var l int
    fn init(rv int, mv int, lv int)
        super.init(rv, mv)
        self.l = lv

fn main()
    let x = Leaf(rv: 1, mv: 2, lv: 3)
    println(f"{x.r}")
    println(f"{x.m}")
    println(f"{x.l}")
    "#,
        "1\n2\n3",
    );
}
