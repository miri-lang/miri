// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A call through a trait or abstract-class receiver reaches the body the
//! instance's class gives the method, whichever other traits and abstract
//! bases share its vtable.

use super::utils::*;

#[test]
fn test_trait_receivers_reach_their_own_method_when_a_class_implements_two_traits() {
    // `zeta` sorts after `alpha`, so a slot numbered within `A` alone lands on
    // `alpha` in the class's combined vtable.
    assert_runs_with_output(
        r#"
use system.io

trait A
    fn zeta() int
trait B
    fn alpha() int

class C implements A, B
    fn zeta() int
        return 1
    fn alpha() int
        return 2

fn main()
    let a A = C()
    println(f"{a.zeta()}")
    let b B = C()
    println(f"{b.alpha()}")
"#,
        "1\n2",
    );
}

#[test]
fn test_abstract_receiver_reaches_its_method_beside_an_inherited_trait() {
    // The trait's `greet` sorts before the base's `shout`, so a slot numbered
    // within the base alone lands on `name`.
    assert_runs_with_output(
        r#"
use system.io

trait Named
    fn name(self) String
    fn greet(self) String
        return f"hi {self.name()}"

abstract class Base implements Named
    abstract fn name(self) String
    fn shout(self) String
        return self.greet()

class Dog extends Base
    fn name(self) String
        return "dog"

fn main()
    let d Base = Dog()
    println(d.shout())
    let n Named = Dog()
    println(n.greet())
    println(Dog().greet())
"#,
        "hi dog\nhi dog\nhi dog",
    );
}

#[test]
fn test_class_implementing_three_traits_dispatches_through_each() {
    assert_runs_with_output(
        r#"
use system.io

trait First
    fn mid() int
trait Second
    fn aaa() int
trait Third
    fn zzz() int

class Impl implements First, Second, Third
    fn mid() int
        return 10
    fn aaa() int
        return 20
    fn zzz() int
        return 30

fn main()
    let f First = Impl()
    let s Second = Impl()
    let t Third = Impl()
    println(f"{f.mid()} {s.aaa()} {t.zzz()}")
"#,
        "10 20 30",
    );
}

#[test]
fn test_abstract_receiver_skips_a_static_method_sorting_before_the_dispatched_one() {
    // A static method never takes a vtable slot, so it must not shift the
    // slot of the instance method sorting after it.
    assert_runs_with_output(
        r#"
use system.io

abstract class Shape
    public static fn apex() int
        return 99
    abstract fn area() int
    abstract fn border() int

class Square extends Shape
    fn area() int
        return 16
    fn border() int
        return 4

fn main()
    let s Shape = Square()
    println(f"{s.area()} {s.border()} {Shape.apex()}")
"#,
        "16 4 99",
    );
}

#[test]
fn test_trait_methods_interleaving_alphabetically_dispatch_to_their_own_bodies() {
    // The combined order is b, c, d, e, f, g: every trait's methods sit at
    // different positions than within the trait alone.
    assert_runs_with_output(
        r#"
use system.io

trait Odd
    fn b() int
    fn d() int
    fn f() int
trait Even
    fn c() int
    fn e() int
    fn g() int

class Both implements Odd, Even
    fn b() int
        return 2
    fn c() int
        return 3
    fn d() int
        return 4
    fn e() int
        return 5
    fn f() int
        return 6
    fn g() int
        return 7

fn main()
    let o Odd = Both()
    let e Even = Both()
    println(f"{o.b()} {o.d()} {o.f()} {e.c()} {e.e()} {e.g()}")
"#,
        "2 4 6 3 5 7",
    );
}

#[test]
fn test_classes_sharing_a_trait_but_not_others_dispatch_through_it() {
    // Two classes implement `Shared` beside different other traits, so the
    // slot `Shared.value` takes must hold in both vtables.
    assert_runs_with_output(
        r#"
use system.io

trait Shared
    fn value() int
trait OnlyLeft
    fn aardvark() int
trait OnlyRight
    fn zebra() int

class Left implements Shared, OnlyLeft
    fn value() int
        return 1
    fn aardvark() int
        return 100

class Right implements OnlyRight, Shared
    fn value() int
        return 2
    fn zebra() int
        return 200

fn show(s Shared)
    println(f"{s.value()}")

fn main()
    show(Left())
    show(Right())
    let r OnlyRight = Right()
    let l OnlyLeft = Left()
    println(f"{l.aardvark()} {r.zebra()}")
"#,
        "1\n2\n100 200",
    );
}

#[test]
fn test_trait_receiver_rejects_a_method_only_another_trait_declares() {
    assert_compiler_error(
        r#"
use system.io

trait A
    fn zeta() int
trait B
    fn alpha() int

class C implements A, B
    fn zeta() int
        return 1
    fn alpha() int
        return 2

fn main()
    let a A = C()
    println(f"{a.alpha()}")
"#,
        "alpha",
    );
}

#[test]
fn test_child_trait_receiver_reaches_the_parent_traits_method() {
    assert_runs_with_output(
        r#"
use system.io

trait Parent
    fn foo() int
trait Child extends Parent
    fn bar() int

class Impl implements Child
    fn foo() int
        return 111
    fn bar() int
        return 222

fn main()
    let c Child = Impl()
    println(f"{c.foo()} {c.bar()}")
"#,
        "111 222",
    );
}

#[test]
fn test_unrelated_traits_declaring_one_method_name_dispatch_to_each_class() {
    // Both traits name `measure`, so it takes one slot every vtable shares.
    assert_runs_with_output(
        r#"
use system.io

trait Ruler
    fn measure() int
    fn zigzag() int
trait Scale
    fn apex() int
    fn measure() int

class Tape implements Ruler
    fn measure() int
        return 31
    fn zigzag() int
        return 32

class Balance implements Scale
    fn apex() int
        return 41
    fn measure() int
        return 42

fn main()
    let r Ruler = Tape()
    let s Scale = Balance()
    println(f"{r.measure()} {r.zigzag()} {s.apex()} {s.measure()}")
"#,
        "31 32 41 42",
    );
}

#[test]
fn test_class_implementing_a_stdlib_trait_and_a_user_trait_dispatches_through_both() {
    assert_runs_with_output(
        r#"
use system.io
use system.ops

trait Labelled
    fn label() int

class Weight implements Comparable, Labelled
    let grams int
    fn init(grams int)
        self.grams = grams
    public fn compare(other Self) int
        return self.grams - other.grams
    fn label() int
        return 77

fn main()
    let c Comparable = Weight(50)
    println(f"{c.compare(Weight(8))}")
    let l Labelled = Weight(1)
    println(f"{l.label()}")
"#,
        "42\n77",
    );
}

#[test]
fn test_trait_declared_in_an_imported_module_dispatches_beside_a_local_trait() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.shapes.sized\n",
                    "\n",
                    "trait Named\n",
                    "    fn name() int\n",
                    "\n",
                    "class Box implements Sized, Named\n",
                    "    fn width() int\n",
                    "        return 5\n",
                    "    fn name() int\n",
                    "        return 6\n",
                    "\n",
                    "fn main()\n",
                    "    let s Sized = Box()\n",
                    "    let n Named = Box()\n",
                    "    println(f\"{s.width()} {n.name()}\")\n",
                ),
            ),
            (
                "shapes/sized.mi",
                "public trait Sized\n    fn width() int\n",
            ),
        ],
        "5 6",
    );
}

#[test]
fn test_class_implementing_only_a_marker_trait_links_and_runs() {
    assert_runs_with_output(
        r#"
use system.io

trait Marker

class M implements Marker
    fn hello() int
        return 7

fn main()
    let m M = M()
    println(f"{m.hello()}")
"#,
        "7",
    );
}

#[test]
fn test_marker_trait_class_beside_a_dispatching_trait_links_and_runs() {
    assert_runs_with_output(
        r#"
use system.io

trait Marker
trait Other
    fn thing() int

class M implements Marker
    fn hello() int
        return 7

class N implements Other
    fn thing() int
        return 9

fn main()
    let m M = M()
    let n Other = N()
    println(f"{m.hello()} {n.thing()}")
"#,
        "7 9",
    );
}
