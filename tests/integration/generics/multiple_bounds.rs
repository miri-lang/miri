// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A type parameter with several bounds joined by `and`
//! (`T implements Named and Aged`).
//!
//! An argument meets the parameter when it meets every bound, and a body has
//! the methods and operators of each one. A bounded parameter handed on meets
//! the bounds its own bounds entail. An argument missing one bound is refused
//! with that bound named, and a type parameter that takes the name of a type
//! in scope — the trap a comma between two bounds sets — is refused where it
//! is declared.

use super::utils::*;

const PEOPLE: &str = r##"
use system.io

trait Named
    fn name() String

trait Aged
    fn age() int

trait Tagged
    fn tag() String

class Person implements Named, Aged, Tagged
    public var label String
    public var years int
    public fn name() String
        return self.label
    public fn age() int
        return self.years
    public fn tag() String
        return "#" + self.label

class Pet implements Named
    public var label String
    public fn name() String
        return self.label

fn describe<T implements Named and Aged>(x T) String
    return f"{x.name()} is {x.age()}"
"##;

#[test]
fn an_argument_meeting_two_bounds_runs_the_methods_of_both() {
    let code = format!(
        "{PEOPLE}
fn main()
    println(describe(Person(label: \"ada\", years: 36)))
"
    );
    assert_runs_with_output(&code, "ada is 36");
}

#[test]
fn an_argument_meeting_three_bounds_runs_the_methods_of_each() {
    let code = format!(
        "{PEOPLE}
fn badge<T implements Named and Aged and Tagged>(x T) String
    return f\"{{x.tag()}} {{x.name()}} {{x.age()}}\"

fn main()
    println(badge(Person(label: \"bo\", years: 7)))
"
    );
    assert_runs_with_output(&code, "#bo bo 7");
}

#[test]
fn a_body_orders_and_hashes_through_two_stdlib_bounds() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.set
use system.collections.list

fn smallest<T implements Comparable and Hashable>(xs [T]) T
    var low = xs[0]
    for x in xs
        if x < low
            low = x
    return low

fn distinct<T implements Comparable and Hashable>(xs [T]) int
    var seen = Set<T>()
    for x in xs
        seen.add(x)
    return seen.length()

fn same<T implements Comparable and Hashable>(a T, b T) bool
    return a.hash() == b.hash() and not (a < b) and not (b < a)

fn main()
    let ints = List([3, 1, 3, 2])
    let words = List(["pear", "fig", "pear"])
    println(f"{smallest(ints)} {distinct(ints)} {same(4, 4)}")
    println(f"{smallest(words)} {distinct(words)} {same('a', 'b')}")
"#,
        "1 3 true\nfig 2 false",
    );
}

#[test]
fn an_argument_missing_one_bound_is_refused_naming_it() {
    let code = format!(
        "{PEOPLE}
fn main()
    println(describe(Pet(label: \"rex\")))
"
    );
    assert_compiler_error(&code, "does not satisfy constraint implements Aged");
}

#[test]
fn a_class_argument_missing_one_bound_is_refused_naming_it() {
    let code = format!(
        "{PEOPLE}
class Card<T implements Named and Aged>
    v T

fn main()
    let c = Card<Pet>(v: Pet(label: \"rex\"))
    println(c.v.name())
"
    );
    assert_compiler_error(
        &code,
        "Type Pet does not satisfy constraint implements Aged",
    );
}

#[test]
fn a_bounded_parameter_is_handed_on_to_a_two_bound_function_and_class() {
    let code = format!(
        "{PEOPLE}
class Card<T implements Named and Aged>
    v T

fn card<T implements Named and Aged and Tagged>(x T) Card<T>
    return Card<T>(v: x)

fn shout<T implements Tagged and Named and Aged>(x T) String
    return describe(x) + \"!\"

fn main()
    let p = Person(label: \"cy\", years: 3)
    println(shout(p))
    println(describe(card(p).v))
"
    );
    assert_runs_with_output(&code, "cy is 3!\ncy is 3");
}

#[test]
fn a_parameter_whose_trait_extends_both_bounds_is_handed_on() {
    let code = format!(
        "{PEOPLE}
trait Living extends Named, Aged
    fn alive() bool

class Cat implements Living
    public fn name() String
        return \"tom\"
    public fn age() int
        return 4
    public fn alive() bool
        return true

fn check<T implements Living>(x T) String
    return describe(x)

fn main()
    println(check(Cat()))
"
    );
    assert_runs_with_output(&code, "tom is 4");
}

#[test]
fn a_parameter_meeting_only_one_bound_is_not_handed_on() {
    let code = format!(
        "{PEOPLE}
fn relay<T implements Named>(x T) String
    return describe(x)

fn main()
    println(relay(Pet(label: \"rex\")))
"
    );
    assert_compiler_error(&code, "does not satisfy constraint implements Aged");
}

#[test]
fn a_comma_between_two_bounds_is_refused_with_the_and_spelling() {
    assert_compiler_error(
        r#"
use system.io

fn big<T implements Comparable, Hashable>(a T, b T) T
    if a < b
        return b
    return a

fn main()
    println(f"{big(3, 2)}")
"#,
        "`Hashable` is a trait; to require both, write `T implements Comparable and Hashable`",
    );
}

#[test]
fn a_type_parameter_named_like_a_class_is_refused() {
    assert_compiler_error(
        r#"
use system.io

class Point
    public var x int

fn keep<Point>(p Point) Point
    return p

fn main()
    println(f"{keep(3)}")
"#,
        "type parameter `Point` hides the class `Point`",
    );
}

#[test]
fn a_type_declared_with_several_traits_is_refused_for_each() {
    let code = format!(
        "{PEOPLE}
type Badge implements Named and Aged

fn main()
    println(\"x\")
"
    );
    assert_compiler_error(&code, "'Badge' cannot implement trait 'Named'");
    assert_compiler_error(&code, "'Badge' cannot implement trait 'Aged'");
}

#[test]
fn a_generic_class_calls_the_methods_of_both_bounds_on_its_field() {
    let code = format!(
        "{PEOPLE}
class Card<T implements Named and Aged>
    v T
    fn init(v T)
        self.v = v
    public fn show() String
        return f\"{{self.v.name()}}:{{self.v.age()}}\"

fn main()
    let c = Card<Person>(Person(label: \"di\", years: 9))
    println(c.show())
"
    );
    assert_runs_with_output(&code, "di:9");
}

#[test]
fn a_bound_list_after_extends_takes_a_subclass_implementing_the_trait() {
    assert_runs_with_output(
        r#"
use system.io

trait Named
    fn name() String

class Animal
    public var legs int
    public fn count() int
        return self.legs

class Dog extends Animal implements Named
    public fn name() String
        return "dog"

fn walk<T extends Animal and Named>(x T) String
    return f"{x.name()} on {x.count()}"

fn main()
    println(walk(Dog(legs: 4)))
"#,
        "dog on 4",
    );
}

#[test]
fn a_bound_list_after_extends_refuses_a_subclass_missing_the_trait() {
    assert_compiler_error(
        r#"
use system.io

trait Named
    fn name() String

class Animal
    public var legs int

class Cat extends Animal

fn walk<T extends Animal and Named>(x T) String
    return x.name()

fn main()
    println(walk(Cat(legs: 4)))
"#,
        "does not satisfy constraint extends Named",
    );
}

const DOUBLER: &str = r##"
use system.io

trait Doubler
    fn twice() Self

trait Named
    fn name() String

class Word implements Doubler, Named
    public var text String
    public fn twice() Self
        return Word(text: self.text + self.text)
    public fn name() String
        return self.text
"##;

#[test]
fn a_self_returning_method_keeps_the_other_bounds_of_its_receiver() {
    let code = format!(
        "{DOUBLER}
fn f<T implements Doubler and Named>(x T) String
    return x.twice().name()

fn main()
    println(f(Word(text: \"ab\")))
"
    );
    assert_runs_with_output(&code, "abab");
}

#[test]
fn a_self_returning_method_keeps_the_other_bounds_of_a_class_field() {
    let code = format!(
        "{DOUBLER}
class Box<T implements Named and Doubler>
    public var v T
    public fn show() String
        let doubled = self.v.twice()
        return doubled.name()

fn main()
    let b = Box<Word>(v: Word(text: \"q\"))
    println(b.show())
"
    );
    assert_runs_with_output(&code, "qq");
}

#[test]
fn a_parameter_extending_another_and_a_trait_is_read_as_the_other() {
    assert_runs_with_output(
        r#"
use system.io

trait Named
    fn name() String

class A implements Named
    public fn name() String
        return "a"

fn up<T, U extends T and Named>(u U) T
    return u

fn main()
    let a A = up<A, A>(A())
    println(a.name())
"#,
        "a",
    );
}

#[test]
fn two_bounds_declaring_one_member_with_different_signatures_are_refused() {
    assert_compiler_error(
        r#"
use system.io

trait Named
    fn name() String

trait Labelled
    fn name() int

fn show<T implements Named and Labelled>(x T) String
    return "x"

fn main()
    println("x")
"#,
        "the bounds `Named` and `Labelled` of `T` both declare `name` with different signatures",
    );
}

#[test]
fn two_bounds_declaring_one_member_with_the_same_signature_are_accepted() {
    assert_runs_with_output(
        r#"
use system.io

trait Named
    fn name() String

trait Titled
    fn name() String

class P implements Named, Titled
    public fn name() String
        return "p"

fn show<T implements Titled and Named>(x T) String
    return x.name()

fn main()
    println(show(P()))
"#,
        "p",
    );
}

#[test]
fn a_bound_list_naming_two_unrelated_classes_is_refused() {
    assert_compiler_error(
        r#"
use system.io

class Animal
    public var legs int

class Robot
    public var bolts int

fn walk<T extends Animal and Robot>(x T) int
    return 1

fn main()
    println("x")
"#,
        "a type extends at most one class",
    );
}

#[test]
fn a_bound_list_naming_two_related_classes_is_refused() {
    assert_compiler_error(
        r#"
use system.io

class Animal
    public var legs int

class Dog extends Animal

fn walk<T extends Animal and Dog>(x T) int
    return 1

fn main()
    println("x")
"#,
        "`T` names two classes, `Animal` and `Dog`",
    );
}

#[test]
fn a_bound_list_naming_one_class_and_traits_is_accepted() {
    assert_runs_with_output(
        r#"
use system.io

trait Named
    fn name() String

trait Aged
    fn age() int

class Animal
    public var legs int
    public fn count() int
        return self.legs

class Dog extends Animal implements Named, Aged
    public fn name() String
        return "dog"
    public fn age() int
        return 3

fn walk<T extends Animal and Named and Aged>(x T) String
    return f"{x.name()} {x.age()} {x.count()}"

fn main()
    println(walk(Dog(legs: 4)))
"#,
        "dog 3 4",
    );
}

#[test]
fn a_repeated_bound_is_refused_as_redundant() {
    assert_compiler_error(
        r#"
use system.io

fn big<T implements Comparable and Comparable>(a T, b T) T
    if a < b
        return b
    return a

fn main()
    println(f"{big(3, 2)}")
"#,
        "the bound `Comparable` is written twice for `T`",
    );
}

#[test]
fn a_type_set_written_with_is_beside_another_bound_is_refused() {
    assert_compiler_error(
        r#"
use system.io
use system.math

fn f<T is Real and Hashable>(a T) int
    return a.hash()

fn main()
    println("x")
"#,
        "a type set (`is`) is not combined with other bounds",
    );
}

#[test]
fn a_type_set_written_with_implements_beside_another_bound_is_refused() {
    assert_compiler_error(
        r#"
use system.io
use system.math

fn f<T implements Real and Hashable>(a T) int
    return a.hash()

fn main()
    println("x")
"#,
        "a type set (`is Real`) is not combined with other bounds",
    );
}

#[test]
fn a_member_no_bound_declares_is_refused_naming_every_bound() {
    let code = format!(
        "{PEOPLE}
fn f<T implements Named and Aged>(a T) String
    return a.color()

fn main()
    println(\"x\")
"
    );
    assert_compiler_error(
        &code,
        "no bound of `T` declares `color`: `T implements Named and Aged`",
    );
}
