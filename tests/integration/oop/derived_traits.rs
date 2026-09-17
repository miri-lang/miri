// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A class implements every trait its traits extend.
//!
//! Conformance makes a class implementing `trait Ranked extends Comparable`
//! declare `compare`, so the class orders its values exactly as one naming
//! `Comparable` directly would: under the four ordering operators, in a sorted
//! `List`, and through any class that extends it. The same holds for the other
//! capabilities an operator or a container asks for.

use super::utils::*;

const RANKED_CARD: &str = r#"
use system.collections.list

trait Ranked extends Comparable
    fn rank() int

class Card implements Ranked
    v int

    fn init(v int)
        self.v = v

    fn rank() int
        return self.v

    public fn compare(other Self) int
        return self.v - other.v
"#;

fn program(base: &str, rest: &str) -> String {
    format!("{base}\n{rest}")
}

#[test]
fn test_class_implementing_a_trait_that_extends_comparable_orders_under_every_operator() {
    assert_runs_with_output(
        &program(
            RANKED_CARD,
            r#"
fn main()
    let a = Card(1)
    let b = Card(2)
    println(f"{a < b} {a <= b} {a > b} {a >= b} {b < a}")
"#,
        ),
        "true true false false false",
    );
}

#[test]
fn test_list_of_a_class_implementing_a_trait_that_extends_comparable_sorts() {
    assert_runs_with_output(
        &program(
            RANKED_CARD,
            r#"
fn main()
    var l = List<Card>()
    l.push(Card(20))
    l.push(Card(10))
    l.push(Card(30))
    l.sort()
    println(f"{l[0].v},{l[1].v},{l[2].v}")
"#,
        ),
        "10,20,30",
    );
}

#[test]
fn test_generic_body_orders_and_array_sorts_a_class_implementing_a_trait_that_extends_comparable() {
    assert_runs_with_output(
        &program(
            RANKED_CARD,
            r#"
fn bigger<T>(a T, b T) T
    if a > b
        return a
    return b

fn main()
    var arr = [Card(5), Card(2), Card(9)]
    arr.sort()
    println(f"{bigger(Card(3), Card(7)).v} {arr[0].v}{arr[1].v}{arr[2].v}")
"#,
        ),
        "7 259",
    );
}

#[test]
fn test_comparable_two_traits_up_orders_the_class() {
    assert_runs_with_output(
        r#"
use system.collections.list

trait Ranked extends Comparable
    fn rank() int

trait Suited extends Ranked
    fn suit() int

class Card implements Suited
    v int

    fn init(v int)
        self.v = v

    fn rank() int
        return self.v

    fn suit() int
        return 0

    public fn compare(other Self) int
        return other.v - self.v

fn main()
    var l = List<Card>()
    l.push(Card(20))
    l.push(Card(10))
    l.push(Card(30))
    l.sort()
    println(f"{Card(1) < Card(2)} {l[0].v},{l[1].v},{l[2].v}")
"#,
        "false 30,20,10",
    );
}

#[test]
fn test_subclass_of_a_class_implementing_a_trait_that_extends_comparable_orders_and_sorts() {
    assert_runs_with_output(
        &program(
            RANKED_CARD,
            r#"
class Face extends Card
    fn init(v int)
        super.init(v)

fn main()
    var l = List<Face>()
    l.push(Face(20))
    l.push(Face(10))
    l.push(Face(30))
    l.sort()
    println(f"{Face(1) < Face(2)} {Face(2) >= Face(1)} {l[0].v},{l[1].v},{l[2].v}")
"#,
        ),
        "true true 10,20,30",
    );
}

#[test]
fn test_class_implementing_a_trait_that_extends_equatable_compares_by_its_equals() {
    // `equals` looks only at the last digit, so a structural comparison of the
    // field would answer differently for 3 and 13.
    assert_runs_with_output(
        r#"
use system.ops

trait Keyed extends Equatable
    fn key() int

class Code implements Keyed
    v int

    fn init(v int)
        self.v = v

    fn key() int
        return self.v % 10

    public fn equals(other Self) bool
        return self.v % 10 == other.v % 10

fn main()
    let a = Code(3)
    let b = Code(13)
    let c = Code(4)
    println(f"{a == b} {a != b} {a == c}")
"#,
        "true false false",
    );
}

#[test]
fn test_set_of_a_class_implementing_a_trait_that_extends_equatable_matches_by_its_equals() {
    assert_runs_with_output(
        r#"
use system.ops
use system.collections.set

trait Keyed extends Equatable
    fn key() int

class Code implements Keyed
    v int

    fn init(v int)
        self.v = v

    fn key() int
        return self.v % 10

    public fn equals(other Self) bool
        return self.v % 10 == other.v % 10

fn main()
    var s = Set<Code>()
    s.add(Code(3))
    s.add(Code(13))
    s.add(Code(4))
    println(f"{s.length()}")
"#,
        "2",
    );
}

#[test]
fn test_class_implementing_a_trait_that_extends_addable_adds_through_its_concat() {
    assert_runs_with_output(
        r#"
use system.ops

trait Summable extends Addable
    fn amount() int

class Money implements Summable
    cents int

    fn init(cents int)
        self.cents = cents

    fn amount() int
        return self.cents

    public fn concat(other Self) Self
        return Money(self.cents + other.cents)

fn main()
    let total = Money(150) + Money(275)
    println(f"{total.cents}")
"#,
        "425",
    );
}

#[test]
fn test_class_implementing_a_trait_that_extends_multiplicable_multiplies_through_its_repeat() {
    assert_runs_with_output(
        r#"
use system.ops

trait Scalable extends Multiplicable
    fn size() int

class Word implements Scalable
    n int

    fn init(n int)
        self.n = n

    fn size() int
        return self.n

    public fn repeat(count int) Self
        return Word(self.n * count)

fn main()
    let w = Word(4) * 3
    println(f"{w.n}")
"#,
        "12",
    );
}

#[test]
fn test_cloning_a_list_of_a_class_implementing_a_trait_that_extends_cloneable_uses_its_clone() {
    // `clone` adds 100, so a copy that shared or copied the elements' bytes
    // would print the originals.
    assert_runs_with_output(
        r#"
use system.collections.list

trait Duplicable extends Cloneable
    fn tag() int

class Token implements Duplicable
    v int

    fn init(v int)
        self.v = v

    fn tag() int
        return 1

    public fn clone() Self
        return Token(self.v + 100)

fn main()
    var l = List<Token>()
    l.push(Token(1))
    l.push(Token(2))
    let c = l.clone()
    println(f"{c[0].v},{c[1].v},{l[0].v}")
"#,
        "101,102,1",
    );
}

#[test]
fn test_class_implementing_a_trait_that_does_not_extend_comparable_has_no_ordering() {
    assert_compiler_error(
        r#"
trait Ranked
    fn rank() int

class Card implements Ranked
    v int

    fn init(v int)
        self.v = v

    fn rank() int
        return self.v

fn main()
    println(f"{Card(1) < Card(2)}")
"#,
        "has no ordering",
    );
}
