// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method called on a value whose type is a trait-bounded parameter
//! (`x T` where `T extends Closable`) runs the implementing class's method, or
//! the trait's default where the class declares none — in a generic function,
//! in a generic caller handing the value on, and in a generic class holding it.

use super::utils::*;

const CLOSABLES: &str = r#"
trait Closable
    fn close() String
        return "default"

class Handle implements Closable
    public var id int

class Own implements Closable
    public var id int
    public fn close() String
        return "own"
"#;

#[test]
fn a_bounded_parameter_runs_the_class_method_or_the_trait_default() {
    let code = format!(
        "{CLOSABLES}
fn shut<T extends Closable>(x T) String
    return x.close()

fn twice<T extends Closable>(x T) String
    return shut(x) + shut(x)

fn main()
    println(shut(Handle(id: 1)))
    println(shut(Own(id: 2)))
    println(twice(Own(id: 3)))
"
    );
    assert_heap_guard_output(&code, "default\nown\nownown");
}

#[test]
fn a_generic_class_calls_the_method_its_bounded_field_answers() {
    let code = format!(
        "{CLOSABLES}
class Box<T extends Closable>
    v T
    fn init(v T)
        self.v = v
    public fn shut() String
        return self.v.close()

fn main()
    let a = Box<Handle>(Handle(id: 1))
    let b = Box<Own>(Own(id: 2))
    println(f\"{{a.shut()}} {{b.shut()}}\")
"
    );
    assert_heap_guard_output(&code, "default own");
}

/// The value handed to a bounded parameter is released where the body is
/// done with it, and its own drop hook runs exactly once.
#[test]
fn a_bounded_parameter_releases_the_value_through_its_drop_hook_once() {
    assert_heap_guard_output(
        r#"
trait Closable
    fn close() String

class Res implements Closable
    public var id int
    public fn close() String
        return "closed"
    fn drop()
        println(f"drop {self.id}")

fn finish<T extends Closable>(x T)
    println(x.close())

fn main()
    finish(Res(id: 7))
    println("end")
"#,
        "closed\ndrop 7\nend",
    );
}

/// A class implementing a generic trait at one instantiation, handed to a
/// parameter spelled as that trait.
const OP_OF_FOO: &str = r#"
struct Foo
    x int

trait Op<U>
    fn get() U

class Box implements Op<Foo>
    fn get() Foo
        return Foo(1)

fn use_op<X>(o Op<X>) X
    return o.get()
"#;

#[test]
fn a_trait_parameter_is_pinned_by_the_clause_the_argument_implements_it_through() {
    let code = format!(
        "{OP_OF_FOO}
fn main()
    let b = Box()
    let r = use_op(b)
    println(f\"{{r.x}}\")
"
    );
    assert_heap_guard_output(&code, "1");
}

#[test]
fn an_explicit_type_argument_matching_the_implemented_clause_is_accepted() {
    let code = format!(
        "{OP_OF_FOO}
fn main()
    let b = Box()
    let r = use_op<Foo>(b)
    println(f\"{{r.x}}\")
"
    );
    assert_heap_guard_output(&code, "1");
}

#[test]
fn an_explicit_type_argument_the_argument_does_not_implement_is_refused() {
    let code = format!(
        "{OP_OF_FOO}
fn main()
    let b = Box()
    let r = use_op<String>(b)
    println(r)
"
    );
    assert_compiler_error(
        &code,
        "Type mismatch for argument 'o': expected Op<String>, got Box",
    );
}

#[test]
fn a_class_is_refused_where_its_trait_is_declared_at_other_arguments() {
    let code = format!(
        "{OP_OF_FOO}
fn take(o Op<String>) String
    return o.get()

fn main()
    println(take(Box()))
"
    );
    assert_compiler_error(
        &code,
        "Type mismatch for argument 'o': expected Op<String>, got Box",
    );
}

/// A class reaching `Op<Foo>` through its base class's clause, two links up.
const DERIVED_OP: &str = r#"
struct Foo
    x int

trait Op<U>
    fn get() U

class Base implements Op<Foo>
    fn get() Foo
        return Foo(1)

class Derived extends Base
    fn tag() int
        return 2

fn use_op<X>(o Op<X>) X
    return o.get()
"#;

#[test]
fn a_trait_parameter_is_pinned_through_two_links_of_clauses() {
    let code = format!(
        "{DERIVED_OP}
fn main()
    let r = use_op(Derived())
    println(f\"{{r.x}}\")
"
    );
    assert_heap_guard_output(&code, "1");
}

#[test]
fn a_class_two_links_below_a_trait_is_refused_at_other_arguments() {
    let code = format!(
        "{DERIVED_OP}
fn take(o Op<String>) String
    return o.get()

fn main()
    println(take(Derived()))
"
    );
    assert_compiler_error(
        &code,
        "Type mismatch for argument 'o': expected Op<String>, got Derived",
    );
}

/// A type argument is read and written through the instance alike, so a
/// sink of dogs is no sink of animals: storing an animal in it would hand its
/// dog readers something that is not one.
#[test]
fn a_trait_is_invariant_in_its_type_arguments() {
    assert_compiler_error(
        r#"
class Animal
    n int
    fn init(n int)
        self.n = n

class Dog extends Animal
    tag String
    fn init(t String)
        super.init(1)
        self.tag = t

trait Sink<T>
    fn put(a T) String

class DogSink implements Sink<Dog>
    fn put(a Dog) String
        return a.tag

fn feed(s Sink<Animal>) String
    return s.put(Animal(5))

fn main()
    println(feed(DogSink()))
"#,
        "Type mismatch for argument 's': expected Sink<Animal>, got DogSink",
    );
}

#[test]
fn a_class_is_invariant_in_its_type_arguments() {
    assert_compiler_error(
        r#"
class Animal
    n int
    fn init(n int)
        self.n = n

class Dog extends Animal
    tag String
    fn init(t String)
        super.init(1)
        self.tag = t

class Box<T>
    v T?
    fn set(a T)
        self.v = a

fn fill(b Box<Animal>)
    b.set(Animal(5))

fn main()
    let b = Box<Dog>()
    fill(b)
"#,
        "Type mismatch for argument 'b': expected Box<Animal>, got Box<Dog>",
    );
}

/// A class extending a generic class without its arguments pins that class's
/// parameter to nothing, so no instantiation of it is the one the class is.
#[test]
fn extending_a_generic_class_without_its_arguments_is_refused() {
    assert_compiler_error(
        r#"
struct Foo
    x int

trait Op<U>
    fn get() U

class A<X> implements Op<X>
    val X
    fn init(v X)
        self.val = v
    fn get() X
        return self.val

class B extends A
    fn init()
        super.init(Foo(7))

fn main()
    println(f"{B().get().x}")
"#,
        "'A' takes the type argument `X`, which 'B' does not write",
    );
}

/// A bare clause would be read by parameter name, while the supertype's
/// members are laid out by parameter position: `B<Y, X> extends A` would read
/// `A<X, Y>`'s `x` at `B`'s first argument.
#[test]
fn a_bare_clause_is_refused_even_where_the_parameter_names_match() {
    assert_compiler_error(
        r#"
class A<X, Y>
    x X
    y Y
    fn gx() X
        return self.x

class B<Y, X> extends A

fn main()
    let b = B<String, int>(x: 5, y: "s" + "t")
    println(f"{b.gx()}")
"#,
        "'A' takes the type arguments `X`, `Y`, which 'B' does not write: write the type each one is given — `A<..., ...>`",
    );
}

/// A bound the supertype declares on its parameter is checked only against
/// the argument a clause writes, so a bare clause would leave it unchecked.
#[test]
fn a_bare_clause_over_a_bounded_parameter_is_refused() {
    assert_compiler_error(
        r#"
trait Named
    fn name() String

class A<X implements Named>
    val X
    fn init(v X)
        self.val = v
    fn nm() String
        return self.val.name()

class B<X> extends A
    fn init(v X)
        super.init(v)

fn main()
    let b = B<int>(3)
    println(b.nm())
"#,
        "'A' takes the type argument `X`, which 'B' does not write",
    );
}

const NAMED: &str = r#"
trait Named
    fn name() String

class P
    v int
    fn init(v int)
        self.v = v

class Box<T>
    v T?

fn make<T implements Named>() Box<T>
    return Box<T>()

fn call_it<T implements Named>(x T) String
    return x.name()
"#;

#[test]
fn a_bound_is_checked_where_an_argument_binds_the_parameter() {
    let code = format!(
        "{NAMED}
fn main()
    println(call_it(P(3)))
"
    );
    assert_compiler_error(&code, "Type P does not satisfy constraint implements Named");
}

#[test]
fn a_bound_is_checked_where_an_explicit_type_argument_binds_the_parameter() {
    let code = format!(
        "{NAMED}
fn main()
    let b = make<P>()
    println(\"unreachable\")
"
    );
    assert_compiler_error(&code, "Type P does not satisfy constraint implements Named");
}

#[test]
fn a_bound_is_checked_where_the_location_binds_the_parameter() {
    let code = format!(
        "{NAMED}
fn main()
    let b Box<P> = make()
    println(\"unreachable\")
"
    );
    assert_compiler_error(&code, "Type P does not satisfy constraint implements Named");
}

const BOUNDED_CLASS: &str = r#"
trait Named
    fn name() String

class P implements Named
    fn name() String
        return "p"

class A<X implements Named>
    val X
    fn init(v X)
        self.val = v
    fn nm() String
        return self.val.name()
"#;

/// A class's parameter bound is checked where the class is constructed, as
/// it is in an annotation: a body is never compiled at a type the bound
/// excludes.
#[test]
fn a_construction_at_an_argument_breaking_the_class_bound_is_refused() {
    assert_compiler_error(
        &format!(
            "{BOUNDED_CLASS}{}",
            r#"
fn main()
    let b = A<int>(3)
    let s = b.nm()
"#
        ),
        "Type int does not satisfy constraint implements Named",
    );
}

#[test]
fn a_clause_argument_breaking_the_class_bound_is_refused() {
    assert_compiler_error(
        &format!(
            "{BOUNDED_CLASS}{}",
            r#"
class B extends A<int>
    fn init()
        super.init(3)

fn main()
    let b = B()
"#
        ),
        "does not satisfy constraint implements Named",
    );
}

const BOUNDED_TRAIT: &str = r#"
use system.io

trait Measured
    fn size() int

class Plain
    var w int
    fn init(w int)
        self.w = w

class Item implements Measured
    var w int
    fn init(w int)
        self.w = w
    public fn size() int: self.w

trait Container<T implements Measured>
    fn get() T
    fn weight() int
        return self.get().size()
"#;

/// A trait's parameter bound is checked where an `implements` clause pins
/// it, as it is in an `extends` clause: the trait's defaults are never
/// compiled at a type the bound excludes.
#[test]
fn an_implements_clause_argument_breaking_the_trait_bound_is_refused() {
    assert_compiler_error(
        &format!(
            "{BOUNDED_TRAIT}{}",
            r#"
class Holder implements Container<Plain>
    var it Plain
    fn init(it Plain)
        self.it = it
    public fn get() Plain: self.it

fn main()
    println(f"{Holder(Plain(4)).weight()}")
"#
        ),
        "Type Plain does not satisfy constraint implements Measured",
    );
}

#[test]
fn an_implements_clause_naming_an_unbounded_parameter_is_refused() {
    assert_compiler_error(
        &format!(
            "{BOUNDED_TRAIT}{}",
            r#"
class Holder<U> implements Container<U>
    var it U
    fn init(it U)
        self.it = it
    public fn get() U: self.it

fn main()
    println(f"{Holder<Item>(Item(4)).weight()}")
"#
        ),
        "Type U does not satisfy constraint implements Measured",
    );
}

#[test]
fn a_trait_extends_clause_argument_breaking_the_trait_bound_is_refused() {
    assert_compiler_error(
        &format!(
            "{BOUNDED_TRAIT}{}",
            r#"
trait Box extends Container<Plain>
    fn label() String

fn main()
    println("unreachable")
"#
        ),
        "Type Plain does not satisfy constraint implements Measured",
    );
}

#[test]
fn clauses_meeting_the_trait_bound_run_the_default() {
    assert_runs_with_output(
        &format!(
            "{BOUNDED_TRAIT}{}",
            r#"
class Holder<U implements Measured> implements Container<U>
    var it U
    fn init(it U)
        self.it = it
    public fn get() U: self.it

trait Shelf extends Container<Item>
    fn label() String

class Board implements Shelf
    public fn get() Item: Item(5)
    public fn label() String: "board"

fn main()
    println(f"[{Holder<Item>(Item(4)).weight()} {Board().weight()}]")
"#
        ),
        "[4 5]",
    );
}

/// Whether a clause argument meets a bound depends on the traits it
/// implements, so the check waits until every type of the module is defined:
/// a clause may name a class declared below it.
#[test]
fn clauses_may_name_a_class_declared_below_them() {
    assert_runs_with_output(
        r#"
use system.io

trait Measured
    fn size() int

class Base<T implements Measured>
    var it T
    fn init(it T)
        self.it = it

class Child extends Base<Late>
    fn init(it Late)
        super.init(it)

trait Container<T implements Measured>
    fn get() T
    fn weight() int
        return self.get().size()

class Holder implements Container<Late>
    var it Late
    fn init(it Late)
        self.it = it
    public fn get() Late: self.it

trait Shelf extends Container<Late>
    fn label() String

class Late implements Measured
    var w int
    fn init(w int)
        self.w = w
    public fn size() int: self.w

fn main()
    println(f"[{Child(Late(2)).it.size()} {Holder(Late(3)).weight()}]")
"#,
        "[2 3]",
    );
}

#[test]
fn a_construction_at_an_argument_meeting_the_class_bound_runs() {
    assert_runs_with_output(
        &format!(
            "{BOUNDED_CLASS}{}",
            r#"
use system.io

fn main()
    let b = A<P>(P())
    println(b.nm())
"#
        ),
        "p",
    );
}
