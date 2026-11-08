// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `Self` written inside a trait stands for the class implementing it.
//!
//! A class implements a trait method by declaring `Self` as itself, wherever
//! the trait writes it, while a parameter the trait types as the trait itself
//! stays the trait. A call through a value of a trait type cannot know which
//! class that value is, so a method that takes a `Self`, or returns one inside
//! another type, is refused there; a type parameter bounded by the trait names
//! the class and is the repair.

use super::utils::*;

/// Assert `code` is refused with `MER_TYP_089`, the message carrying
/// `message`.
fn assert_refused_through_trait_value(code: &str, message: &str) {
    let output = crate::utils::miri_check(code).output();
    assert!(
        output.contains("MER_TYP_089") && output.contains(message),
        "expected MER_TYP_089 with '{message}' in:\n{output}"
    );
}

const SAME_TRAIT: &str = r#"
use system.io

trait Same
    fn same(other Self) bool

class A implements Same
    var x float
    fn init(x float)
        self.x = x
    public fn same(other A) bool
        self.x == other.x

class B implements Same
    var label String
    fn init(label String)
        self.label = label
    public fn same(other B) bool
        self.label == other.label
"#;

#[test]
fn test_implementer_may_not_narrow_a_parameter_the_trait_types_as_the_trait() {
    assert_compiler_error(
        r#"
use system.io

trait Shape
    fn area() float
    fn bigger(other Shape) bool

class Circle implements Shape
    var r float
    fn init(r float)
        self.r = r
    public fn area() float
        3.0 * self.r * self.r
    public fn bigger(other Circle) bool
        self.r > other.r

fn main()
    let c = Circle(r: 1.0)
    println(f"{c.bigger(Circle(r: 2.0))}")
"#,
        "Method 'bigger' in class 'Circle' does not match trait 'Shape' signature",
    );
}

#[test]
fn test_implementer_keeps_a_parameter_the_trait_types_as_the_trait() {
    assert_heap_guard_output(
        r#"
use system.io

trait Shape
    fn area() float
    fn bigger(other Shape) bool

class Circle implements Shape
    var r float
    fn init(r float)
        self.r = r
    public fn area() float
        3.0 * self.r * self.r
    public fn bigger(other Shape) bool
        self.area() > other.area()

class Square implements Shape
    var side float
    fn init(side float)
        self.side = side
    public fn area() float
        self.side * self.side
    public fn bigger(other Shape) bool
        self.area() > other.area()

fn compare(a Shape, b Shape) bool
    a.bigger(b)

fn main()
    println(f"{compare(Circle(r: 1.0), Square(side: 2.0))}")
    println(f"{compare(Square(side: 2.0), Circle(r: 1.0))}")
"#,
        "false\ntrue",
    );
}

#[test]
fn test_call_taking_self_through_a_trait_value_is_refused() {
    assert_refused_through_trait_value(
        &format!(
            "{SAME_TRAIT}
fn check(p Same, q Same) bool
    p.same(q)

fn main()
    println(f\"{{check(A(x: 1.0), B(label: 'b'))}}\")
"
        ),
        "the method `same` takes a `Self`, and a value of the trait `Same` does not say which class that is",
    );
}

#[test]
fn test_bounded_type_parameter_is_the_repair_for_a_self_parameter() {
    assert_heap_guard_output(
        &format!(
            "{SAME_TRAIT}
fn check<T implements Same>(p T, q T) bool
    p.same(q)

fn main()
    println(f\"{{check(A(x: 1.0), A(x: 1.0))}}\")
    println(f\"{{check(B(label: 'b'), B(label: 'c'))}}\")
"
        ),
        "true\nfalse",
    );
}

#[test]
fn test_bounded_type_parameter_refuses_two_different_classes() {
    assert_compiler_error(
        &format!(
            "{SAME_TRAIT}
fn check<T implements Same>(p T, q T) bool
    p.same(q)

fn main()
    println(f\"{{check(A(x: 1.0), B(label: 'b'))}}\")
"
        ),
        "Type mismatch for argument 'q': expected A, got B",
    );
}

#[test]
fn test_trait_default_returns_self_nested_in_a_type_argument() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

trait Seq<T>
    fn size() int
    fn at(i int) T
    fn walker() Walk<T, Self>
        return Walk<T, Self>(source: self)

class Walk<T, S implements Seq<T>>
    var source S
    var index int
    fn init(source S)
        self.source = source
        self.index = 0
    public fn next() T?
        if self.index >= self.source.size()
            return None
        let x = self.source.at(self.index)
        self.index += 1
        x

class Words implements Seq<String>
    var all [String]
    fn init(all [String])
        self.all = all
    public fn size() int
        self.all.length()
    public fn at(i int) String
        self.all[i]

fn main()
    let w = Words(all: List(["a", "b"]))
    let it = w.walker()
    let a = it.next() ?? "-"
    let b = it.next() ?? "-"
    let c = it.next() ?? "-"
    println(f"{a} {b} {c}")
"#,
        "a b -",
    );
}

#[test]
fn test_return_naming_self_inside_another_type_is_refused_through_a_trait_value() {
    assert_refused_through_trait_value(
        r#"
use system.io
use system.collections.list

trait Seq<T>
    fn size() int
    fn at(i int) T
    fn walker() Walk<T, Self>
        return Walk<T, Self>(source: self)

class Walk<T, S implements Seq<T>>
    var source S
    fn init(source S)
        self.source = source

class Words implements Seq<String>
    var all [String]
    fn init(all [String])
        self.all = all
    public fn size() int
        self.all.length()
    public fn at(i int) String
        self.all[i]

fn main()
    let s Seq<String> = Words(all: List(["a", "b"]))
    let it = s.walker()
"#,
        "the method `walker` returns `Self` inside another type, and a value of the trait `Seq` does not say which class that is",
    );
}

#[test]
fn test_bare_self_return_through_a_trait_value_gives_the_trait_value() {
    assert_heap_guard_output(
        r#"
use system.io

trait Named
    fn name() int
    fn me() Self
        return self
    fn maybe() Self?
        return self

class A implements Named
    var n int
    fn init(n int)
        self.n = n
    public fn name() int
        self.n

fn main()
    let x Named = A(n: 4)
    let y = x.me()
    println(f"{y.name()}")
    let z = x.maybe()
    if let Some(v) = z
        println(f"{v.name()}")
"#,
        "4\n4",
    );
}

#[test]
fn test_trait_default_taking_self_runs_on_a_concrete_receiver() {
    assert_heap_guard_output(
        r#"
use system.io

trait Named
    fn name() int
    fn twice() int
        let me = self
        return me.name() + self.name()
    fn pick(other Self) Self
        if other.name() > self.name(): return other
        return self

class A implements Named
    var n int
    fn init(n int)
        self.n = n
    public fn name() int
        self.n

fn main()
    let a = A(n: 3)
    println(f"{a.twice()}")
    let b = a.pick(A(n: 5))
    println(f"{b.name()}")
"#,
        "6\n5",
    );
}

#[test]
fn test_concrete_receiver_reads_a_trait_default_self_return_as_its_class() {
    assert_heap_guard_output(
        r#"
use system.io

trait Named
    fn name() int
    fn pick(other Self) Self
        if other.name() > self.name(): return other
        return self

class A implements Named
    var n int
    fn init(n int)
        self.n = n
    public fn name() int
        self.n

fn main()
    let a = A(n: 3)
    let b A = a.pick(A(n: 5))
    println(f"{b.n}")
"#,
        "5",
    );
}

#[test]
fn test_generic_trait_bounded_parameter_reads_self_as_the_parameter() {
    assert_heap_guard_output(
        r#"
use system.io

trait Pick<T>
    fn val() T
    fn better(other Self) bool

class P implements Pick<int>
    let v int
    fn init(v int)
        self.v = v
    public fn val() int: self.v
    public fn better(other P) bool: self.v > other.v

fn best<X implements Pick<int>>(a X, b X) bool
    a.better(b)

fn main()
    println(f"{best(P(4), P(2))}")
    println(f"{best(P(1), P(2))}")
"#,
        "true\nfalse",
    );
}

#[test]
fn test_equality_on_two_equatable_trait_values_is_refused() {
    assert_refused_through_trait_value(
        r#"
use system.io

class A implements Equatable
    var x float
    fn init(x float)
        self.x = x
    public fn equals(other A) bool
        self.x == other.x

fn eq(p Equatable, q Equatable) bool
    p == q

fn main()
    println(f"{eq(A(x: 1.0), A(x: 1.0))}")
"#,
        "a value of the trait `Equatable` does not say which class that is",
    );
}

#[test]
fn test_ordering_on_two_comparable_trait_values_is_refused_without_naming_the_trait_as_implementer()
{
    assert_refused_through_trait_value(
        r#"
use system.io

class W implements Comparable
    let g int
    fn init(g int)
        self.g = g
    public fn compare(other Self) int
        return self.g - other.g

fn lt(a Comparable, b Comparable) bool
    a < b

fn main()
    println(f"{lt(W(5), W(3))}")
"#,
        "a value of the trait `Comparable` does not say which class that is",
    );
}

#[test]
fn test_equality_on_bounded_type_parameter_calls_equals() {
    assert_heap_guard_output(
        r#"
use system.io

class A implements Equatable
    var x float
    fn init(x float)
        self.x = x
    public fn equals(other A) bool
        self.x == other.x

fn eq<T implements Equatable>(p T, q T) bool
    p == q

fn main()
    println(f"{eq(A(x: 1.0), A(x: 1.0))}")
    println(f"{eq(A(x: 1.0), A(x: 2.0))}")
"#,
        "true\nfalse",
    );
}

#[test]
fn test_implementer_may_write_self_or_its_own_name_for_a_self_parameter() {
    assert_heap_guard_output(
        r#"
use system.io

trait Same
    fn same(other Self) bool

class A implements Same
    var x int
    fn init(x int)
        self.x = x
    public fn same(other Self) bool
        self.x == other.x

class B implements Same
    var x int
    fn init(x int)
        self.x = x
    public fn same(other B) bool
        self.x == other.x

fn main()
    println(f"{A(1).same(A(1))} {B(1).same(B(2))}")
"#,
        "true false",
    );
}

#[test]
fn test_implementer_may_not_take_another_class_for_a_self_parameter() {
    assert_compiler_error(
        r#"
use system.io

trait Same
    fn same(other Self) bool

class B
    var x int
    fn init(x int)
        self.x = x

class A implements Same
    var x int
    fn init(x int)
        self.x = x
    public fn same(other B) bool
        self.x == other.x

fn main()
    println(f"{A(1).same(B(1))}")
"#,
        "Method 'same' in class 'A' does not match trait 'Same' signature",
    );
}

#[test]
fn test_ordering_on_trait_values_without_an_order_names_a_bounded_parameter() {
    assert_compiler_error(
        r#"
use system.io

trait Shape
    fn area() float

class Square implements Shape
    var side float
    fn init(side float)
        self.side = side
    public fn area() float
        self.side * self.side

fn lt(a Shape, b Shape) bool
    a < b

fn main()
    println(f"{lt(Square(1.0), Square(2.0))}")
"#,
        "take the values at one type parameter whose bounds include Comparable",
    );
}

#[test]
fn test_compound_assignment_through_an_addable_trait_value_is_refused() {
    assert_refused_through_trait_value(
        r#"
use system.io
use system.ops

class Money implements Addable
    let cents int
    fn init(cents int)
        self.cents = cents
    public fn concat(other Self) Self
        return Money(self.cents + other.cents)

fn grow(total Addable, more Addable) Addable
    var sum = total
    sum += more
    return sum

fn main()
    let m = grow(Money(1), Money(2))
"#,
        "`+` runs `concat`, which takes a `Self`, and a value of the trait `Addable` does not say which class that is",
    );
}

/// `Holder` holds a value of any type, beside a field of its own.
const HOLDER: &str = r#"
use system.io
use system.collections.list

class Holder<T>
    var item T
    var n int
    fn init(item T)
        self.item = item
        self.n = 42

trait Labelled
    fn label() String
"#;

fn with_holder(rest: &str) -> String {
    format!("{HOLDER}\n{rest}")
}

#[test]
fn test_self_held_in_a_generic_class_is_read_back_as_self() {
    assert_heap_guard_output(
        &with_holder(
            r#"
trait Wrapped extends Labelled
    fn go() String
        let h = Holder<Self>(self)
        let y = h.item
        y.label()

class A implements Wrapped
    var x int
    fn init(x int)
        self.x = x
    public fn label() String
        f"A{self.x}"

fn main()
    println(A(7).go())
"#,
        ),
        "A7",
    );
}

#[test]
fn test_self_held_in_a_generic_class_does_not_gain_the_holders_fields() {
    assert_compiler_error(
        &with_holder(
            r#"
trait Wrapped extends Labelled
    fn go() int
        let h = Holder<Self>(self)
        let y = h.item
        y.n

class A implements Wrapped
    var x int
    fn init(x int)
        self.x = x
    public fn label() String
        f"A{self.x}"

fn main()
    println(f"{A(7).go()}")
"#,
        ),
        "Trait 'Wrapped' has no method 'n'",
    );
}

#[test]
fn test_generic_class_instance_at_self_is_built_for_each_implementer() {
    assert_heap_guard_output(
        &with_holder(
            r#"
trait Wrapped extends Labelled
    fn go() int
        let h = Holder<Self>(self)
        h.n

class A implements Wrapped
    var x int
    fn init(x int)
        self.x = x
    public fn label() String
        f"A{self.x}"

fn main()
    println(f"{A(7).go()}")
"#,
        ),
        "42",
    );
}

#[test]
fn test_list_of_self_takes_self_in_a_default() {
    assert_heap_guard_output(
        &with_holder(
            r#"
trait Listed extends Labelled
    fn go() int
        let xs = List<Self>()
        xs.push(self)
        xs.push(self)
        xs.length()

class A implements Listed
    var x int
    fn init(x int)
        self.x = x
    public fn label() String
        f"A{self.x}"

fn main()
    println(f"{A(7).go()}")
"#,
        ),
        "2",
    );
}

#[test]
fn test_default_naming_self_inherited_by_a_generic_class_runs() {
    assert_runs_with_output(
        r#"
use system.io

class Holder<T>
    var item T
    var n int
    fn init(item T)
        self.item = item
        self.n = 42

trait Doubler
    fn label() String
    fn go() int
        Holder<Self>(self).n

class G<U> implements Doubler
    var u U
    fn init(u U)
        self.u = u
    public fn label() String
        "G"

fn main()
    println(f"{G<int>(1).go()}")
"#,
        "42",
    );
}

/// A trait whose default takes and returns `Self` inside other types,
/// implemented by a generic class.
const SAME_PICK: &str = r#"
use system.io
use system.collections.list

trait Same
    fn same(other Self) bool
    fn pick(others [Self]) Self?
        for o in others
            if self.same(o)
                return Some(o)
        None

class Box<T> implements Same
    var t T
    var n int
    fn init(t T, n int)
        self.t = t
        self.n = n
    public fn same(other Box<T>) bool
        self.n == other.n
"#;

#[test]
fn test_default_taking_a_list_of_self_on_a_generic_class_runs_from_a_default() {
    assert_runs_with_output(
        &format!(
            "{SAME_PICK}
trait Labeled
    fn label() String
    fn go() bool
        let a = Box<Self>(self, 1)
        let r = a.pick(List([Box<Self>(self, 2), Box<Self>(self, 1)]))
        match r
            Some(v): v.t.label() == self.label() and v.n == 1
            None: false

class L implements Labeled
    var s String
    fn init(s String)
        self.s = s
    public fn label() String
        self.s

fn main()
    let l = L(\"q\")
    println(f\"{{l.go()}}\")
"
        ),
        "true",
    );
}

#[test]
fn test_default_taking_a_list_of_self_on_a_generic_class_runs_from_a_generic_function() {
    assert_runs_with_output(
        &format!(
            "{SAME_PICK}
class L
    var s String
    fn init(s String)
        self.s = s

fn go<S>(x S) bool
    let a = Box<S>(x, 1)
    let r = a.pick(List([Box<S>(x, 2), Box<S>(x, 1)]))
    match r
        Some(v): v.n == 1
        None: false

fn main()
    let l = L(\"q\")
    println(f\"{{go(l)}}\")
"
        ),
        "true",
    );
}

/// The message a trait type meeting a bound refuses with.
const SAME_BOUND_REFUSAL: &str =
    "a value of the trait `Same` cannot stand for a type parameter bounded by `Same`: the method `same` takes a `Self`";

#[test]
fn test_trait_values_do_not_bind_a_parameter_bounded_by_a_trait_taking_self() {
    assert_refused_through_trait_value(
        &format!(
            "{SAME_TRAIT}
fn check<T implements Same>(p T, q T) bool
    p.same(q)

fn main()
    let a Same = A(x: 1.0)
    let b Same = B(label: 'b')
    println(f\"{{check(a, b)}}\")
"
        ),
        SAME_BOUND_REFUSAL,
    );
}

#[test]
fn test_trait_written_as_the_type_argument_of_a_bound_taking_self_is_refused() {
    assert_refused_through_trait_value(
        &format!(
            "{SAME_TRAIT}
fn check<T implements Same>(p T, q T) bool
    p.same(q)

fn main()
    let b Same = B(label: 'b')
    println(f\"{{check<Same>(A(x: 1.0), b)}}\")
"
        ),
        SAME_BOUND_REFUSAL,
    );
}

#[test]
fn test_trait_as_the_argument_of_a_generic_class_bounded_by_a_trait_taking_self_is_refused() {
    assert_refused_through_trait_value(
        &format!(
            "{SAME_TRAIT}
class Pair<T implements Same>
    var l T
    var r T
    fn init(l T, r T)
        self.l = l
        self.r = r
    public fn eq() bool
        self.l.same(self.r)

fn main()
    let a Same = A(x: 1.0)
    let b Same = B(label: 'b')
    let p = Pair<Same>(a, b)
    println(f\"{{p.eq()}}\")
"
        ),
        SAME_BOUND_REFUSAL,
    );
}

#[test]
fn test_equatable_values_do_not_bind_a_parameter_bounded_by_equatable() {
    assert_refused_through_trait_value(
        r#"
use system.io

class A implements Equatable
    var x int
    fn init(x int)
        self.x = x
    public fn equals(other A) bool
        self.x == other.x

class B implements Equatable
    var s int
    fn init(s int)
        self.s = s
    public fn equals(other B) bool
        self.s == other.s

fn eq<T implements Equatable>(a T, b T) bool
    a.equals(b)

fn main()
    let a Equatable = A(1)
    let b Equatable = B(1)
    println(f"{eq(a, b)}")
"#,
        "a value of the trait `Equatable` cannot stand for a type parameter bounded by `Equatable`: the method `equals` takes a `Self`",
    );
}

#[test]
fn test_searching_a_list_of_trait_values_that_compare_by_self_is_refused() {
    assert_refused_through_trait_value(
        r#"
use system.io
use system.collections.list

trait Shape extends Equatable
    fn area() int

class A implements Shape
    var s String
    fn init(s String)
        self.s = s
    public fn area() int
        1
    public fn equals(other A) bool
        self.s == other.s

fn main()
    let a Shape = A("hello")
    let l = List<Shape>()
    l.push(a)
    println(f"{l.contains(a)}")
"#,
        "`==` runs `equals`, which takes a `Self`, and a value of the trait `Shape` does not say which class that is",
    );
}

#[test]
fn test_searching_a_list_of_equatable_values_is_refused() {
    assert_refused_through_trait_value(
        r#"
use system.io
use system.collections.list

class A implements Equatable
    var x int
    fn init(x int)
        self.x = x
    public fn equals(other A) bool
        self.x == other.x

fn main()
    let xs = List<Equatable>()
    xs.push(A(1))
    println(f"{xs.contains(A(1))}")
"#,
        "`==` runs `equals`, which takes a `Self`, and a value of the trait `Equatable` does not say which class that is",
    );
}

#[test]
fn test_equality_on_optional_equatable_values_is_refused() {
    assert_refused_through_trait_value(
        r#"
use system.io

class A implements Equatable
    var x int
    fn init(x int)
        self.x = x
    public fn equals(other A) bool
        self.x == other.x

fn same(a Equatable?, b Equatable?) bool
    a == b

fn main()
    println(f"{same(A(1), A(1))}")
"#,
        "a value of the trait `Equatable` does not say which class that is",
    );
}

#[test]
fn test_trait_returning_only_a_bare_self_still_meets_its_own_bound() {
    assert_heap_guard_output(
        r#"
use system.io

trait Node
    fn val() int
    fn maybe(flag bool) Self?
        if flag
            return self
        return None

class A implements Node
    var n int
    fn init(n int)
        self.n = n
    public fn val() int
        self.n

fn total<T implements Node>(x T) int
    match x.maybe(true)
        Some(v): v.val()
        None: 0

fn main()
    let n Node = A(5)
    println(f"{total(n)}")
"#,
        "5",
    );
}

#[test]
fn test_override_of_an_inherited_default_may_not_narrow_its_self_parameter() {
    assert_compiler_error(
        r#"
use system.io

trait Doubler
    fn label() String
    fn other_label(xs Self) String
        xs.label()

class Base implements Doubler
    var x int
    fn init(x int)
        self.x = x
    public fn label() String
        f"B{self.x}"

class Derived extends Base
    var y int
    fn init(x int)
        super.init(x)
        self.y = 77
    public fn label() String
        f"D{self.x}"
    public fn other_label(xs Derived) String
        f"{xs.y}"

fn main()
    let b Base = Derived(1)
    println(b.other_label(Base(9)))
"#,
        "Method 'other_label' has incompatible parameter type for 'xs' (position 1): expected Base, got Derived",
    );
}

#[test]
fn test_closure_in_a_default_returns_self() {
    assert_heap_guard_output(
        r#"
use system.io

trait Named
    fn name() String
    fn shout() String
        let f = fn() Self: self
        f().name()

class A implements Named
    var x int
    fn init(x int)
        self.x = x
    public fn name() String
        f"A{self.x}"

fn main()
    println(A(3).shout())
"#,
        "A3",
    );
}

/// A trait whose methods take `Self` inside an optional, a list and a
/// function type.
const SHAPES_OF_SELF: &str = r#"
use system.io
use system.collections.list

trait Same
    fn same(other Self?) bool
    fn lst(o [Self]) int
    fn cb(f fn(Self) int) int

class A implements Same
    var x int
    fn init(x int)
        self.x = x
    public fn same(other A?) bool
        match other
            Some(o): self.x == o.x
            None: false
    public fn lst(o [A]) int
        o.length()
    public fn cb(f fn(A) int) int
        f(self)
"#;

#[test]
fn test_self_inside_a_function_type_resolves_on_a_concrete_receiver() {
    assert_heap_guard_output(
        &format!(
            "{SHAPES_OF_SELF}
fn main()
    let a = A(3)
    println(f\"{{a.cb(fn(v A) int: v.x * 2)}} {{a.same(A(3))}} {{a.lst(List([A(1), A(2)]))}}\")
"
        ),
        "6 true 2",
    );
}

#[test]
fn test_self_inside_a_function_type_in_a_class_is_the_class() {
    assert_heap_guard_output(
        r#"
use system.io

class C
    var v int
    fn init(v int)
        self.v = v
    public fn apply(f fn(x Self) int) int
        f(self)

fn main()
    println(f"{C(4).apply(fn(x C) int: x.v * 2)}")
"#,
        "8",
    );
}

#[test]
fn test_method_taking_a_function_of_self_through_a_trait_value_is_refused() {
    assert_refused_through_trait_value(
        &format!(
            "{SHAPES_OF_SELF}
fn c(p Same) int
    p.cb(fn(a A) int: 1)

fn main()
    println(f\"{{c(A(1))}}\")
"
        ),
        "the method `cb` takes a `Self`",
    );
}

#[test]
fn test_method_taking_an_optional_self_through_a_trait_value_is_refused() {
    assert_refused_through_trait_value(
        &format!(
            "{SHAPES_OF_SELF}
fn c(p Same, q Same) bool
    p.same(q)

fn main()
    println(f\"{{c(A(1), A(1))}}\")
"
        ),
        "the method `same` takes a `Self`",
    );
}

#[test]
fn test_method_taking_a_list_of_self_through_a_trait_value_is_refused() {
    assert_refused_through_trait_value(
        &format!(
            "{SHAPES_OF_SELF}
fn c(p Same) int
    p.lst(List([A(1)]))

fn main()
    println(f\"{{c(A(1))}}\")
"
        ),
        "the method `lst` takes a `Self`",
    );
}

#[test]
fn test_call_taking_self_on_an_element_of_a_list_of_trait_values_is_refused() {
    assert_refused_through_trait_value(
        &format!(
            "{SAME_TRAIT}
use system.collections.list

fn main()
    let ps = List<Same>()
    ps.push(A(x: 1.0))
    println(f\"{{ps[0].same(ps[0])}}\")
"
        ),
        "the method `same` takes a `Self`",
    );
}

#[test]
fn test_method_value_taking_self_through_a_trait_value_is_refused() {
    assert_refused_through_trait_value(
        &format!(
            "{SAME_TRAIT}
fn main()
    let s Same = A(x: 1.0)
    let f = s.same
"
        ),
        "the method `same` takes a `Self`",
    );
}

#[test]
fn test_ordering_on_values_of_a_trait_extending_comparable_is_refused() {
    assert_refused_through_trait_value(
        r#"
use system.io

trait Ranked extends Comparable
    fn rank() int

class W implements Ranked
    let g int
    fn init(g int)
        self.g = g
    public fn rank() int
        self.g
    public fn compare(other Self) int
        self.g - other.g

fn lt(a Ranked, b Ranked) bool
    a < b

fn main()
    println(f"{lt(W(5), W(3))}")
"#,
        "`<` runs `compare`, which takes a `Self`, and a value of the trait `Ranked` does not say which class that is",
    );
}

#[test]
fn test_subtraction_assignment_on_trait_values_is_refused() {
    assert_compiler_error(
        r#"
use system.io
use system.ops

class Money implements Addable
    let cents int
    fn init(cents int)
        self.cents = cents
    public fn concat(other Self) Self
        Money(self.cents + other.cents)

fn shrink(total Addable, less Addable) Addable
    var sum = total
    sum -= less
    sum

fn main()
    let m = shrink(Money(3), Money(2))
"#,
        "Invalid types for arithmetic operation: Addable and Addable",
    );
}

#[test]
fn test_default_of_a_parent_trait_taking_self_runs_through_a_bounded_parameter() {
    assert_heap_guard_output(
        r#"
use system.io

trait Same
    fn same(other Self) bool
    fn differs(other Self) bool
        not self.same(other)

trait Sub extends Same
    fn tag() int

class A implements Sub
    var x int
    fn init(x int)
        self.x = x
    public fn same(other A) bool
        self.x == other.x
    public fn tag() int
        self.x

fn apart<T implements Sub>(p T, q T) bool
    p.differs(q)

fn main()
    println(f"{apart(A(1), A(2))} {apart(A(1), A(1))}")
"#,
        "true false",
    );
}

#[test]
fn test_default_calling_another_default_reads_self_as_the_class() {
    assert_heap_guard_output(
        r#"
use system.io

trait Same
    fn same(other Self) bool
    fn differs(other Self) bool
        not self.same(other)
    fn both_differ(a Self, b Self) bool
        self.differs(a) and self.differs(b)

class A implements Same
    var x int
    fn init(x int)
        self.x = x
    public fn same(other A) bool
        self.x == other.x

fn main()
    println(f"{A(1).both_differ(A(2), A(3))} {A(1).both_differ(A(1), A(3))}")
"#,
        "true false",
    );
}

/// `K` answers `equals` and `hash` through a trait's defaults only.
const KEYED_BY_DEFAULTS: &str = r#"
use system.io
use system.collections.set
use system.collections.map

trait Keyed extends Equatable, Hashable
    fn key() int
    fn equals(other Self) bool
        self.key() == other.key()
    fn hash() int
        self.key()

class K implements Keyed
    var v int
    fn init(v int)
        self.v = v
    public fn key() int
        self.v
"#;

#[test]
fn test_set_identifies_elements_by_a_trait_default_equals_and_hash() {
    assert_heap_guard_output(
        &format!(
            "{KEYED_BY_DEFAULTS}
fn main()
    let s = {{K(1), K(1), K(2)}}
    println(f\"{{s.length()}}\")
"
        ),
        "2",
    );
}

#[test]
fn test_map_identifies_keys_by_a_trait_default_equals_and_hash() {
    assert_heap_guard_output(
        &format!(
            "{KEYED_BY_DEFAULTS}
fn main()
    let m = Map<K, int>()
    m.set(K(1), 10)
    m.set(K(1), 20)
    println(f\"{{m.length()}} {{m[K(1)]}}\")
"
        ),
        "1 20",
    );
}

#[test]
fn test_method_value_through_a_parameter_bounded_by_a_generic_trait_is_refused() {
    assert_compiler_error(
        r#"
use system.io

trait Ord2<T>
    fn cmp(o T) int

class P implements Ord2<P>
    var v int
    fn init(v int)
        self.v = v
    public fn cmp(o P) int
        self.v - o.v

fn f<T implements Ord2<P>>(a T) int
    let g = a.cmp
    0

fn main()
    println(f"{f(P(1))}")
"#,
        "the method `cmp` is read as a value",
    );
}

const RENAMED_SINK: &str = r#"
use system.io

trait Sink<T>
    fn put(x T?) int
    fn put2(x T?, y T?) int
        return self.put(x) + self.put(y)

class Bag<U> implements Sink<U>
    var n int
    fn put(y U?) int
        match y
            Some(v): return 1
            None: return 100

class Sub<V> extends Bag<V>
    var m int
    fn put2(a PARAM, b V?) int
        return 7

fn main()
    let s = Sub<int>()
    println(f"{s.put2(None, Some(2))}")
"#;

#[test]
fn test_override_of_a_default_inherited_through_renamed_parameters_reads_them_renamed() {
    assert_runs_with_output(&RENAMED_SINK.replace("PARAM", "V?"), "7");
}

#[test]
fn test_override_of_a_default_inherited_through_renamed_parameters_cannot_narrow() {
    assert_compiler_error(
        &RENAMED_SINK.replace("PARAM", "int?"),
        "Method 'put2' has incompatible parameter type for 'a' (position 1): expected V?, got int?",
    );
}
