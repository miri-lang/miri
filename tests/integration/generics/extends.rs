// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic method reached through `extends` or `implements`, where the
//! receiver's own class pins the declaring type's parameters in its clause
//! rather than at the call. What the body requires of its parameter is judged
//! at the type the clause binds it to.

use super::utils::*;

#[test]
fn sorting_in_an_inherited_method_is_refused_at_an_unordered_struct() {
    assert_compiler_error(
        r#"
use system.collections.list

struct Pt
    x int

class Base<T>
    fn first_sorted(a T, b T) T
        var l = List<T>()
        l.push(a)
        l.push(b)
        l.sort()
        return l[0]

class Sub extends Base<Pt>

fn main()
    let p = Sub().first_sorted(Pt(x: 2), Pt(x: 1))
    println(f"{p.x}")
"#,
        "MER_TYP_075",
    );
}

#[test]
fn arithmetic_in_an_inherited_method_is_refused_at_a_struct() {
    assert_compiler_error(
        r#"
struct Pt
    x int

class Base<T>
    fn add(a T, b T) T
        return a + b

class Sub extends Base<Pt>

fn main()
    let p = Sub().add(Pt(x: 1), Pt(x: 2))
    println(f"{p.x}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn arithmetic_two_extends_clauses_up_is_refused_at_a_struct() {
    assert_compiler_error(
        r#"
struct Pt
    x int

class Base<T>
    fn add(a T, b T) T
        return a + b

class Mid<U> extends Base<U>

class Sub extends Mid<Pt>

fn main()
    let p = Sub().add(Pt(x: 1), Pt(x: 2))
    println(f"{p.x}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn arithmetic_through_a_renamed_parameter_is_refused_where_the_subclass_is_pinned() {
    assert_compiler_error(
        r#"
struct Pt
    x int

class Base<T>
    fn add(a T, b T) T
        return a + b

class Sub<U> extends Base<U>

fn main()
    let p = Sub<Pt>().add(Pt(x: 1), Pt(x: 2))
    println(f"{p.x}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_inherited_arithmetic_runs_at_the_type_the_clause_binds() {
    assert_runs_with_output(
        r#"
class Base<T>
    fn add(a T, b T) T
        return a + b

class Mid<U> extends Base<U>

class Sub extends Mid<int>

fn main()
    println(f"{Sub().add(20, 22)}")
"#,
        "42",
    );
}

#[test]
fn test_inherited_sort_runs_at_an_ordered_type() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Base<T>
    fn first_sorted(a T, b T) T
        var l = List<T>()
        l.push(a)
        l.push(b)
        l.sort()
        return l[0]

class Sub extends Base<int>

fn main()
    println(f"{Sub().first_sorted(9, 4)}")
"#,
        "4",
    );
}

#[test]
fn arithmetic_in_a_trait_default_is_refused_where_a_plain_class_binds_the_trait() {
    assert_compiler_error(
        r#"
struct Pt
    x int

trait Op<T>
    fn add(a T, b T) T
        return a + b

class Impl implements Op<Pt>

fn main()
    let p = Impl().add(Pt(x: 1), Pt(x: 2))
    println(f"{p.x}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn arithmetic_in_a_generic_enum_method_is_refused_at_a_struct() {
    assert_compiler_error(
        r#"
struct Pt
    x int

enum Pair<T>
    Both(T, T)

    fn sum() T
        match self
            Pair.Both(a, b): a + b

fn main()
    let p Pair<Pt> = Pair.Both(Pt(x: 1), Pt(x: 2))
    println(f"{p.sum().x}")
"#,
        "Invalid types for arithmetic operation",
    );
}

#[test]
fn test_arithmetic_in_a_generic_enum_method_runs_at_int() {
    assert_runs_with_output(
        r#"
enum Pair<T>
    Both(T, T)

    fn sum() T
        match self
            Pair.Both(a, b): a + b

fn main()
    let p Pair<int> = Pair.Both(20, 22)
    println(f"{p.sum()}")
"#,
        "42",
    );
}

#[test]
fn arithmetic_in_a_trait_default_is_refused_through_a_trait_typed_receiver() {
    assert_compiler_error(
        r#"
struct Pt
    x int

trait Op<T>
    fn add(a T, b T) T
        return a + b

class Impl<T> implements Op<T>

fn main()
    let o Op<Pt> = Impl<Pt>()
    let p = o.add(Pt(x: 1), Pt(x: 2))
    println(f"{p.x}")
"#,
        "Invalid types for arithmetic operation",
    );
}

/// The call's type is read at the receiver's type argument, so the result is
/// an `int` that prints, not a bare `T` with no members.
#[test]
fn test_a_trait_default_through_a_trait_typed_receiver_returns_the_bound_type() {
    assert_runs_with_output(
        r#"
struct Pt
    x int

trait Op<T>
    fn keep(a T) T
        return a

class Impl<T> implements Op<T>

fn main()
    let o Op<Pt> = Impl<Pt>()
    let p = o.keep(Pt(x: 42))
    println(f"{p.x}")
"#,
        "42",
    );
}

/// `class Impl implements Op<int>` fixes the trait's `T` in its clause, so the
/// copy of the default `Impl` inherits is lowered at `int`. Lowered at a bare
/// `T` the local is an unknown managed type and is released twice.
#[test]
fn test_a_trait_default_runs_at_the_type_a_plain_class_binds() {
    assert_runs_with_output(
        r#"
trait Op<T>
    fn keep(a T, _b T) T
        var x T = a
        return x

class Impl implements Op<int>

fn main()
    println(f"{Impl().keep(42, 7)}")
"#,
        "42",
    );
}

#[test]
fn test_a_trait_default_holds_a_string_where_a_plain_class_binds_it() {
    assert_runs_with_output(
        r#"
trait Op<T>
    fn keep(a T, _b T) T
        var x T = a
        return x

class Impl implements Op<String>

fn main()
    println(Impl().keep("a" + "b", "c" + "d"))
"#,
        "ab",
    );
}

/// The clause that pins the trait's `T` can sit on a base class: the default
/// runs at the `float` `Sub`'s `extends` clause passes up to its concrete base.
#[test]
fn test_a_trait_default_runs_at_the_type_a_base_class_binds() {
    assert_runs_with_output(
        r#"
trait Op<T>
    fn keep(a T, _b T) T
        var x T = a
        return x

class Base<U> implements Op<U>

class Sub extends Base<float>

fn main()
    println(f"{Sub().keep(1.5, 2.5)}")
"#,
        "1.5",
    );
}

/// Overwriting a local the default holds at the implementor's `String`
/// releases the value it replaced exactly once and hands the new one back.
#[test]
fn test_a_trait_default_overwrites_a_string_local_where_a_plain_class_binds_it() {
    assert_runs_with_output(
        r#"
trait Op<T>
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

class Impl implements Op<String>

fn main()
    println(Impl().keep("a" + "b", "c" + "d"))
"#,
        "cd",
    );
}

/// The same overwrite at an unmanaged `bool`: nothing is released, and the
/// trait's shared body is not compiled at a bare `T` nothing calls it at.
#[test]
fn test_a_trait_default_overwrites_a_bool_local_where_a_plain_class_binds_it() {
    assert_runs_with_output(
        r#"
trait Op<T>
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

class Impl implements Op<bool>

fn main()
    println(f"{Impl().keep(true, false)}")
"#,
        "false",
    );
}

/// A call through a trait-typed parameter reaches the default through the
/// implementor's vtable, whose slot names the copy lowered at the `String` the
/// `extends` clause pins — not the trait's shared body at a bare `T`.
#[test]
fn test_a_trait_default_reached_through_a_vtable_runs_at_the_bound_type() {
    assert_runs_with_output(
        r#"
trait Op<T>
    fn keep(a T, b T) T
        var x T = a
        x = b
        return x

class Base<U> implements Op<U>

class Words extends Base<String>

fn show(o Op<String>)
    println(o.keep("p" + "q", "r" + "s"))

fn main()
    show(Words())
"#,
        "rs",
    );
}

/// An abstract class's method calls a default on its own `self`, which names
/// the trait's shared body: it is still compiled because a call reaches it.
#[test]
fn test_a_trait_default_an_abstract_caller_names_is_still_compiled() {
    assert_runs_with_output(
        r#"
trait Describe
    fn describe() String
        return "shape"

abstract class Shape implements Describe
    fn show() String
        return self.describe()

class Square extends Shape

fn main()
    println(Square().show())
"#,
        "shape",
    );
}

/// Releasing an abstract base runs the drop hook its trait supplies. The
/// base's drop thunk names the trait's shared body, a reference no MIR call
/// carries, so that body is compiled on the strength of the thunk alone.
#[test]
fn test_a_trait_default_drop_hook_an_abstract_base_inherits_runs() {
    assert_runs_with_output(
        r#"
use system.io

trait Closer
    fn drop(self)
        println("closer drop")

abstract class A implements Closer
    var id int

class B extends A

fn main()
    let a A = B(id: 1)
    println(f"{a.id}")
"#,
        "1\ncloser drop",
    );
}

/// When two traits both default `who`, the first one the class lists supplies
/// it, whether the call is static or goes through a vtable.
#[test]
fn test_the_first_listed_trait_default_answers_static_and_virtual_calls() {
    assert_runs_with_output(
        r#"
use system.io

trait A
    fn who(self) String
        return "A"
trait B
    fn who(self) String
        return "B"

class Box implements A, B
    var x int

fn main()
    println(Box(x: 1).who())
    let a A = Box(x: 1)
    println(a.who())
    let b B = Box(x: 1)
    println(b.who())
"#,
        "A\nA\nA",
    );
}

/// The same rule for a generic class, whose vtable slot names the chosen
/// trait's shared body rather than a copy of its own.
#[test]
fn test_the_first_listed_trait_default_answers_a_generic_class_alike() {
    assert_runs_with_output(
        r#"
use system.io

trait A
    fn who(self) String
        return "A"
trait B
    fn who(self) String
        return "B"

class Box<U> implements A, B
    var x U

fn main()
    println(Box<int>(x: 1).who())
    let a A = Box<int>(x: 1)
    println(a.who())
    let b B = Box<int>(x: 1)
    println(b.who())
"#,
        "A\nA\nA",
    );
}

/// A generic class's inherited default that nothing calls by name is compiled
/// because the class's vtable slot names it.
#[test]
fn test_a_generic_class_default_reached_only_through_a_vtable_slot_runs() {
    assert_runs_with_output(
        r#"
use system.io

trait Op<T>
    fn describe() String
        return "generic-default"

abstract class Base<U> implements Op<U>

class Box<T> extends Base<T>
    v T
    fn init(v T)
        self.v = v

fn show(o Op<int>)
    println(o.describe())

fn main()
    show(Box<int>(9))
"#,
        "generic-default",
    );
}

/// Two generic implementors behind one trait-typed parameter: the one that
/// overrides the default answers with its own body, the other with the trait's.
#[test]
fn test_generic_implementors_answer_a_default_or_their_override_through_vtables() {
    assert_runs_with_output(
        r#"
use system.io

trait Op<T>
    fn describe() String
        return "generic-default"

abstract class Base<U> implements Op<U>

class Box<T> extends Base<T>
    v T
    fn init(v T)
        self.v = v

class Loud<T> extends Base<T>
    v T
    fn init(v T)
        self.v = v
    fn describe() String
        return "loud"

fn show(o Op<int>)
    println(o.describe())

fn main()
    show(Box<int>(9))
    show(Loud<int>(3))
"#,
        "generic-default\nloud",
    );
}

/// A class parameter and a trait parameter may share a name: `Box<T>`
/// implements `Op<int>`, so the trait's `T` is `int` inside the default while
/// the class's own `T` stays whatever `Box` is instantiated at. Each copy
/// lowered for an instantiation keeps the two apart — the constructor stores a
/// `String` field as a `String`, and the default adds at `int`.
#[test]
fn test_a_class_parameter_named_like_the_trait_parameter_keeps_its_instantiation() {
    assert_runs_with_output(
        r#"
trait Op<T>
    fn first() T
    fn pick(b T) T
        return self.first() + b

class Box<T> implements Op<int>
    v T
    w int
    fn init(v T, w int)
        self.v = v
        self.w = w
    fn first() int
        return self.w
    fn value() T
        return self.v

fn main()
    let b = Box<String>("a" + "b", 7)
    println(b.value())
    println(f"{b.pick(9)}")
    let f = Box<float>(1.5, 2)
    println(f"{f.value()} {f.pick(3)}")
"#,
        "ab\n16\n1.5 5",
    );
}

/// A parent trait's parameter may share a name with the implementing class's:
/// `Box<T>` implements `Op<int>`, which extends `P<U>`, so `P`'s `T` is `int`
/// in the default `twice` whatever `Box`'s own `T` is instantiated at. The
/// call's type is read through the same clause chain the lowered default is.
#[test]
fn test_a_parent_trait_parameter_named_like_the_class_parameter_types_the_call_at_its_pin() {
    assert_runs_with_output(
        r#"
use system.io

trait P<T>
    fn get() T
    fn twice() T
        return self.get()

trait Op<U> extends P<U>
    fn tag() int

class Box<T> implements Op<int>
    var v T
    var n int
    fn init(v T, n int)
        self.v = v
        self.n = n
    fn get() int
        return self.n
    fn tag() int
        return 1

fn main()
    let b = Box<String>("s", 7)
    let r = b.twice()
    println(f"{r}")
    let f = Box<float>(2.5, 8)
    println(f"{f.twice()}")
"#,
        "7\n8",
    );
}

/// A default declared on a parent trait returns that trait's own parameter,
/// which reaches the class only through `Op<U> extends P<U>`: the call is
/// typed at the pinned `float`, on a class receiver and on a trait-typed one.
#[test]
fn test_a_parent_trait_default_returns_its_parameter_at_the_implements_pin() {
    assert_runs_with_output(
        r#"
use system.io

trait P<T>
    fn get() T
    fn twice() T
        return self.get()

trait Op<U> extends P<U>
    fn tag() int

class Cell<V> implements Op<float>
    var v V
    var x float
    fn init(v V, x float)
        self.v = v
        self.x = x
    fn get() float
        return self.x
    fn tag() int
        return 1

fn show(o Op<float>)
    println(f"{o.twice()}")

fn main()
    let c = Cell<int>(3, 2.5)
    println(f"{c.twice()}")
    show(c)
"#,
        "2.5\n2.5",
    );
}

const WIDE_BASE: &str = r#"
class Base<T>
    public v T
    public n int
    public s String

    fn init(v T, n int, s String)
        self.v = v
        self.n = n
        self.s = s
"#;

/// An inherited field bound to a 128-bit argument is laid out at that width,
/// so construction, reads and the release of the managed field after it all
/// agree on where each field sits.
#[test]
fn an_inherited_field_at_i128_is_read_and_released_at_its_width() {
    let code = format!(
        "{WIDE_BASE}
class Child<T> extends Base<T>
    fn init(v T, n int, s String)
        super.init(v, n, s)

fn main()
    let a i128 = 170141183460469231731687303715884105727
    let c = Child<i128>(v: a, n: 7, s: \"x\" + \"y\")
    println(f\"{{c.v == a}} {{c.n}} {{c.s}}\")
"
    );
    assert_heap_guard_output(&code, "true 7 xy");
}

/// The child names the parameter differently from the base; the base's field
/// is bound through the `extends` clause, not by matching names.
#[test]
fn an_inherited_field_bound_through_a_renamed_parameter_keeps_its_width() {
    let code = format!(
        "{WIDE_BASE}
class Child<U> extends Base<U>
    fn init(v U, n int, s String)
        super.init(v, n, s)

fn main()
    let a u128 = 340282366920938463463374607431768211455
    let c = Child<u128>(v: a, n: 7, s: \"x\" + \"y\")
    println(f\"{{c.v == a}} {{c.n}} {{c.s}}\")
"
    );
    assert_heap_guard_output(&code, "true 7 xy");
}

/// Three inherited fields, each bound through a parameter the child names
/// differently, at three different widths with a managed one between them.
#[test]
fn three_inherited_fields_bound_through_renamed_parameters_read_back_intact() {
    assert_heap_guard_output(
        r#"
class Base<A, B, C>
    public first A
    public second B
    public third C

    fn init(first A, second B, third C)
        self.first = first
        self.second = second
        self.third = third

class Child<X, Y, Z> extends Base<X, Y, Z>
    fn init(first X, second Y, third Z)
        super.init(first, second, third)

fn main()
    let c = Child<i128, String, u8>(5, "x" + "y", 7)
    let d = Child<float, int, String>(1.5, 7, "p" + "q")
    println(f"{c.first} {c.second} {c.third} {d.first} {d.second} {d.third}")
"#,
        "5 xy 7 1.5 7 pq",
    );
}

#[test]
fn a_wide_field_a_plain_child_pins_through_extends_reads_back_intact() {
    // The child declares no parameters, so its instances carry no type
    // arguments; the base's `value T` is still stored at the 128-bit width the
    // clause binds, and every field after it sits past that width.
    assert_runs_with_output(
        r#"
class Base<T>
    var value T
    var tag i32

    fn init(value T, tag i32)
        self.value = value
        self.tag = tag

class Child extends Base<i128>
    var extra i32

    fn init(value i128, extra i32)
        super.init(value, 3)
        self.extra = extra

fn main()
    let big i128 = 9223372036854775807
    let c = Child(big * 1000, 9)
    let v = c.value
    println(f"{v == big * 1000} {c.tag} {c.extra}")
"#,
        "true 3 9",
    );
}
