// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method of a generic class is used wherever the program reaches it, not
//! only where it is reached in the plainest shape: an instance converted to a
//! trait inside a tuple, a list, a match arm, an optional or a
//! variant payload; a trait default called straight on an instance, which
//! hands the instance to the trait's own methods; an element ordered through a
//! base class whose parameter is another instance; and an equality lifted
//! through an optional. Each refused shape used to compile the method at an
//! argument nobody checked — a string's address compared with an integer.

use super::utils::*;

const CANNOT_COMPARE: &str = "cannot compare String and int";

const LESS_THAN_TEN: &str = r#"
use system.io

trait Lt
    fn lt() bool

class Box<T> implements Lt
    v T
    fn lt() bool
        return self.v < 10
"#;

fn with(declarations: &str, main: &str) -> String {
    format!("{declarations}\n{main}")
}

#[test]
fn a_conversion_inside_a_tuple_is_refused_at_the_instances_argument() {
    assert_compiler_error(
        &with(
            LESS_THAN_TEN,
            r#"
fn main()
    let t (Lt, int) = (Box<String>(v: "x" + "y"), 1)
    println(f"{t.0.lt()}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_conversion_inside_a_tuple_runs_at_a_valid_argument() {
    assert_heap_guard_output(
        &with(
            LESS_THAN_TEN,
            r#"
fn main()
    let t (Lt, int) = (Box<int>(v: 3), 1)
    println(f"{t.0.lt()}")
"#,
        ),
        "true",
    );
}

#[test]
fn a_conversion_into_a_list_element_is_refused_at_the_instances_argument() {
    assert_compiler_error(
        &with(
            LESS_THAN_TEN,
            r#"
use system.collections.list

fn main()
    var l = List<Lt>()
    l.push(Box<String>(v: "x" + "y"))
    println(f"{l[0].lt()}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_conversion_in_a_match_arm_is_refused_at_the_arm() {
    assert_compiler_error(
        &with(
            LESS_THAN_TEN,
            r#"
fn pick(c int) Lt
    return match c
        1: Box<String>(v: "x" + "y")
        _: Box<String>(v: "q")

fn main()
    println(f"{pick(1).lt()}")
"#,
        ),
        "15 |         1: Box<String>",
    );
}

#[test]
fn a_conversion_into_an_optional_is_refused_at_the_instances_argument() {
    assert_compiler_error(
        &with(
            LESS_THAN_TEN,
            r#"
fn main()
    var o Lt? = Box<String>(v: "x" + "y")
    if let Some(x) = o
        println(f"{x.lt()}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_conversion_into_a_variant_payload_is_refused_at_the_instances_argument() {
    assert_compiler_error(
        &with(
            LESS_THAN_TEN,
            r#"
fn f() Result<Lt, String>
    return Result.Ok(Box<String>(v: "x" + "y"))

fn main()
    match f()
        Result.Ok(x): println(f"{x.lt()}")
        Result.Err(e): println(e)
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_conversion_into_a_variant_payload_runs_at_a_valid_argument() {
    assert_heap_guard_output(
        &with(
            LESS_THAN_TEN,
            r#"
fn f() Result<Lt, String>
    return Result.Ok(Box<int>(v: 3))

fn main()
    match f()
        Result.Ok(x): println(f"{x.lt()}")
        Result.Err(e): println(e)
"#,
        ),
        "true",
    );
}

const CHECKED_TRAIT: &str = r#"
use system.io

trait Lt
    fn lt() bool
    fn check() bool
        return self.lt()

class Box<T> implements Lt
    v T
    fn lt() bool
        return self.v < 10
"#;

#[test]
fn a_trait_default_called_on_an_instance_is_refused_where_the_trait_method_it_calls_is_not_met() {
    assert_compiler_error(
        &with(
            CHECKED_TRAIT,
            r#"
fn main()
    let b = Box<String>(v: "x" + "y")
    println(f"{b.check()}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_trait_default_called_on_an_instance_runs_at_a_valid_argument() {
    assert_heap_guard_output(
        &with(
            CHECKED_TRAIT,
            r#"
fn main()
    let b = Box<int>(v: 3)
    println(f"{b.check()}")
"#,
        ),
        "true",
    );
}

const ORDERED_THROUGH_A_BASE: &str = r#"
use system.io
use system.ops
use system.collections.list

class Box<T> implements Comparable
    v T
    public fn compare(_other Self) int
        if self.v < 10
            return -1
        return 1

class Base<P> implements Comparable
    p P
    public fn compare(other Self) int
        if self.p < other.p
            return -1
        return 1

class Child<T> extends Base<Box<T>>
    k int
"#;

#[test]
fn a_sort_ordering_through_a_base_whose_parameter_is_an_instance_is_refused() {
    assert_compiler_error(
        &with(
            ORDERED_THROUGH_A_BASE,
            r#"
fn main()
    var l = List<Child<String>>()
    l.push(Child<String>(p: Box<String>(v: "x" + "y"), k: 1))
    l.push(Child<String>(p: Box<String>(v: "x" + "z"), k: 2))
    l.sort()
    println(l[0].p.v)
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_sort_ordering_through_a_base_runs_at_a_valid_argument() {
    assert_heap_guard_output(
        &with(
            ORDERED_THROUGH_A_BASE,
            r#"
fn main()
    var l = List<Child<int>>()
    l.push(Child<int>(p: Box<int>(v: 3), k: 1))
    l.push(Child<int>(p: Box<int>(v: 20), k: 2))
    l.sort()
    println(f"{l[0].p.v}")
"#,
        ),
        "3",
    );
}

const EQUAL_BOX: &str = r#"
use system.io

class Box<T>
    v T
    public fn equals(_o Self) bool
        return self.v == 10
"#;

#[test]
fn an_equality_lifted_through_an_optional_is_refused_at_the_instances_argument() {
    assert_compiler_error(
        &with(
            EQUAL_BOX,
            r#"
fn main()
    let a Box<String>? = Box<String>(v: "x" + "y")
    let b Box<String>? = Box<String>(v: "x" + "z")
    println(f"{a == b}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn an_equality_lifted_through_an_optional_runs_at_a_valid_argument() {
    assert_heap_guard_output(
        &with(
            EQUAL_BOX,
            r#"
fn main()
    let a Box<int>? = Box<int>(v: 10)
    let b Box<int>? = Box<int>(v: 10)
    println(f"{a == b}")
"#,
        ),
        "true",
    );
}

#[test]
fn a_generic_body_comparing_optionals_is_refused_at_the_instances_argument() {
    assert_compiler_error(
        &with(
            EQUAL_BOX,
            r#"
fn same<X>(a X?, b X?) bool
    return a == b

fn main()
    var a Box<String>? = Box<String>(v: "x" + "y")
    var b Box<String>? = Box<String>(v: "x" + "z")
    println(f"{same<Box<String>>(a, b)}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_conversion_at_the_end_of_a_block_match_arm_is_refused_at_the_arm() {
    assert_compiler_error(
        &with(
            LESS_THAN_TEN,
            r#"
fn pick(c int) Lt
    return match c
        1:
            let s = "x" + "y"
            Box<String>(v: s)
        _:
            let t = "q" + "r"
            Box<String>(v: t)

fn main()
    println(f"{pick(1).lt()}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_conversion_at_the_end_of_a_block_match_arm_runs_at_a_valid_argument() {
    assert_heap_guard_output(
        &with(
            LESS_THAN_TEN,
            r#"
fn pick(c int) Lt
    return match c
        1:
            let s = 3
            Box<int>(v: s)
        _:
            let t = 30
            Box<int>(v: t)

fn main()
    println(f"{pick(1).lt()}")
"#,
        ),
        "true",
    );
}

const HOLDERS: &str = r#"
use system.io
use system.ops
use system.collections.list

class Box<T> implements Comparable
    v T
    public fn compare(_other Self) int
        if self.v < 10
            return -1
        return 1

class Holder<T> implements Comparable
    v T
    public fn compare(other Self) int
        if self.v < other.v
            return -1
        return 1

class Base<P> implements Comparable
    p P
    public fn compare(other Self) int
        if self.p < other.p
            return -1
        return 1
"#;

#[test]
fn an_element_reached_through_a_base_at_a_deeper_instance_is_still_checked() {
    assert_compiler_error(
        &with(
            HOLDERS,
            r#"
class W<T> extends Base<Holder<Holder<Box<T>>>>
    k int

fn main()
    var l = List<Holder<W<String>>>()
    l.sort()
    println(f"{l.length()}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn an_ordering_that_reaches_its_class_at_ever_larger_instances_is_refused() {
    assert_compiler_error(
        r#"
use system.io
use system.ops

class Base<P> implements Comparable
    p P
    public fn compare(other Self) int
        if self.p < other.p
            return -1
        return 1

class K<T> extends Base<K<K<T>>>
    w int

fn less(a K<int>, b K<int>) bool
    return a < b

fn main()
    println("ok")
"#,
        "the instantiation recurses without bound",
    );
}

#[test]
fn an_ordering_through_a_deep_chain_of_instances_is_checked_once_per_instance() {
    assert_type_checks(
        r#"
use system.io
use system.ops

class A<T> implements Comparable
    v int
    public fn compare(other Self) int
        return self.v - other.v
    public fn equals(other Self) bool
        return self.v == other.v
    public fn concat(other Self) Self
        return self
    public fn repeat(n int) Self
        return self

class B<T> extends A<T>
    w int

class C<T> extends B<T>
    z int

fn less(a C<C<C<C<C<C<C<C<int>>>>>>>>, b C<C<C<C<C<C<C<C<int>>>>>>>>) bool
    return a < b

fn main()
    println("ok")
"#,
    );
}

const DEFAULTED: &str = r#"
use system.io

trait Tr
    fn a() bool
    fn b() int
        return 1

class Box<T> implements Tr
    v T
    fn a() bool
        return self.v < 10

class Other implements Tr
    k int
    fn a() bool
        return true
"#;

#[test]
fn a_default_that_never_calls_a_method_is_accepted_beside_an_unrelated_conversion() {
    assert_heap_guard_output(
        &with(
            DEFAULTED,
            r#"
fn main()
    let b = Box<String>(v: "x" + "y")
    println(f"{b.b()}")
    let o Tr = Other(k: 1)
    println(f"{o.a()}")
"#,
        ),
        "1\ntrue",
    );
}

/// A default calling a method on a trait-typed parameter runs the method of
/// whatever instance the caller passed, not its own `self`'s. A default that
/// hands its `self` on as a trait value is where that instance becomes its
/// own, so every method called through the trait runs on it.
const HANDS_ON: &str = r#"
use system.io

trait Tr
    fn a() bool
    fn both(other Tr) bool
        return other.a()
    fn go() bool
        return self.both(self)
    fn me() Tr
        return self
    fn via_alias() bool
        let me = self
        return me.a()
    fn via_helper() bool
        return helper(self)

fn helper(x Tr) bool
    return x.a()

class Box<T> implements Tr
    v T
    fn a() bool
        return self.v < 10

class Other implements Tr
    k int
    fn a() bool
        return true
"#;

#[test]
fn a_default_calling_a_method_on_a_trait_typed_parameter_is_accepted_for_another_instance() {
    assert_heap_guard_output(
        &with(
            HANDS_ON,
            r#"
fn main()
    let b = Box<String>(v: "x" + "y")
    println(f"{b.both(Other(k: 1))}")
"#,
        ),
        "true",
    );
}

#[test]
fn a_default_passing_its_self_to_its_own_parameter_is_refused_at_the_instances_argument() {
    assert_compiler_error(
        &with(
            HANDS_ON,
            r#"
fn main()
    let b = Box<String>(v: "x" + "y")
    println(f"{b.go()}")
"#,
        ),
        "cannot compare String and int",
    );
}

#[test]
fn an_instance_passed_to_a_defaults_trait_typed_parameter_is_refused_at_its_argument() {
    assert_compiler_error(
        &with(
            HANDS_ON,
            r#"
fn main()
    let b = Box<String>(v: "x" + "y")
    println(f"{b.both(b)}")
"#,
        ),
        "cannot compare String and int",
    );
}

#[test]
fn a_default_returning_its_self_as_the_trait_is_refused_at_the_instances_argument() {
    assert_compiler_error(
        &with(
            HANDS_ON,
            r#"
fn main()
    let b = Box<String>(v: "x" + "y")
    let t = b.me()
    println(f"{t.a()}")
"#,
        ),
        "cannot compare String and int",
    );
}

/// The refusal lands on a call that never names the failing method, so its
/// help says which default handed the instance on and through which trait the
/// method is reached.
#[test]
fn a_refusal_reached_by_handing_self_on_names_the_hand_off() {
    assert_compiler_error(
        &with(
            HANDS_ON,
            r#"
fn main()
    let b = Box<String>(v: "x" + "y")
    let t = b.me()
    println(f"{t.a()}")
"#,
        ),
        "'me' hands its 'self', a 'Box<String>', on as a 'Tr', and 'a' is called through 'Tr', \
         so 'a' is compiled at 'Box<String>'",
    );
}

#[test]
fn a_refusal_reached_by_handing_self_on_points_at_the_call_through_the_trait() {
    assert_compiler_error(
        &with(
            HANDS_ON,
            r#"
fn main()
    let b = Box<String>(v: "x" + "y")
    let t = b.me()
    println(f"{t.a()}")
"#,
        ),
        "'a' is called through 'Tr' here",
    );
}

#[test]
fn a_default_calling_through_an_alias_of_its_self_is_refused_at_the_instances_argument() {
    assert_compiler_error(
        &with(
            HANDS_ON,
            r#"
fn main()
    let b = Box<String>(v: "x" + "y")
    println(f"{b.via_alias()}")
"#,
        ),
        "cannot compare String and int",
    );
}

#[test]
fn a_default_passing_its_self_to_a_function_is_refused_at_the_instances_argument() {
    assert_compiler_error(
        &with(
            HANDS_ON,
            r#"
fn main()
    let b = Box<String>(v: "x" + "y")
    println(f"{b.via_helper()}")
"#,
        ),
        "cannot compare String and int",
    );
}

#[test]
fn a_default_handing_on_its_self_runs_at_a_valid_argument() {
    assert_heap_guard_output(
        &with(
            HANDS_ON,
            r#"
fn main()
    let b = Box<int>(v: 3)
    println(f"{b.go()} {b.via_helper()} {b.me().a()}")
"#,
        ),
        "true true true",
    );
}

/// A method declared to return `Self` hands back an instance of the class
/// the default runs for, typed as the trait, so storing it as the trait hands
/// that class's instance on just as storing `self` does.
#[test]
fn a_default_handing_on_what_a_self_returning_method_gave_it_is_refused_at_the_instances_argument()
{
    assert_compiler_error(
        r#"
use system.io

trait Tr
    fn a() bool
    fn dup() Self
    fn go(o Tr) bool
        return o.a()
    fn start() bool
        return self.go(self.dup())

class Box<T> implements Tr
    v T
    fn a() bool
        return self.v < 10
    fn dup() Box<T>
        return Box<T>(v: self.v)

fn main()
    let b = Box<String>(v: "x" + "y")
    println(f"{b.start()}")
"#,
        "cannot compare String and int",
    );
}

/// The value a lambda body ends on is returned at the lambda's declared
/// type, which converts an instance returned as a trait.
#[test]
fn an_instance_a_lambda_returns_as_a_trait_is_refused_at_its_argument() {
    assert_compiler_error(
        r#"
use system.io

trait Tr
    fn a() bool

class Box<T> implements Tr
    v T
    fn a() bool
        return self.v < 10

fn main()
    let b = Box<String>(v: "x" + "y")
    let f = fn() Tr: b
    let t = f()
    println(f"{t.a()}")
"#,
        "cannot compare String and int",
    );
}

#[test]
fn an_instance_a_lambda_returns_as_a_trait_runs_at_a_valid_argument() {
    assert_heap_guard_output(
        r#"
use system.io

trait Tr
    fn a() bool

class Box<T> implements Tr
    v T
    fn a() bool
        return self.v < 10

fn main()
    let b = Box<int>(v: 3)
    let f = fn() Tr: b
    let t = f()
    println(f"{t.a()}")
"#,
        "true",
    );
}

/// A default that reads its `self` without handing it on as a trait value —
/// a local alias, a `match` on it, a return at a written `Self`, which the
/// caller reads at the receiver's own class — converts nothing: a method invalid at
/// the instance and only ever called through the trait on another class is
/// not pinned at that instance.
#[test]
fn a_default_reading_its_self_without_handing_it_on_pins_nothing() {
    let rest = r#"
class Box<T> implements Lt
    v T
    fn a() bool
        return self.v < 10

class Other implements Lt
    fn a() bool
        return true

fn call(t Lt) bool
    return t.a()

fn main()
    let b = Box<String>(v: "x")
    println(f"{b.go()} {call(Other())}")
"#;
    for default in [
        "    fn go() int\n        let s = self\n        return 1\n",
        "    fn go() int\n        match self\n            _: 1\n",
        "    fn me2() Self\n        return self\n    fn go() int\n        let m = self.me2()\n        return 1\n",
    ] {
        assert_runs_with_output(
            &format!("trait Lt\n    fn a() bool\n{default}\n{rest}"),
            "1 true",
        );
    }
}

/// A conversion to a trait compiles, at the converted instance, every method
/// the program calls through that trait, wherever the call is written and
/// whichever instance it runs on: whether a conversion is accepted does not
/// depend on where its value flows.
const CALLED_ELSEWHERE: &str = r#"
use system.io

trait Lt
    fn a() bool
    fn b() bool

fn only_a(o Lt) bool
    return o.a()

class Box<T> implements Lt
    v T
    fn a() bool
        return true
    fn b() bool
        return self.v < 10

class Other implements Lt
    k int
    fn a() bool
        return true
    fn b() bool
        return true
"#;

#[test]
fn a_conversion_is_refused_for_a_method_called_through_the_trait_only_on_another_instance() {
    assert_compiler_error(
        &with(
            CALLED_ELSEWHERE,
            r#"
fn main()
    let o Lt = Other(k: 1)
    let b = Box<String>(v: "x" + "y")
    println(f"{o.b()} {only_a(b)}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

/// The refusal lands on the conversion, which never names the failing method,
/// so its help says why the method is compiled there and a note points at the
/// call through the trait that requires it.
#[test]
fn a_refusal_reached_by_a_conversion_names_the_rule_and_the_call_through_the_trait() {
    let program = with(
        CALLED_ELSEWHERE,
        r#"
fn main()
    let o Lt = Other(k: 1)
    let b = Box<String>(v: "x" + "y")
    println(f"{o.b()} {only_a(b)}")
"#,
    );
    assert_compiler_error(
        &program,
        "converting a 'Box<String>' to 'Lt' compiles every method the program calls through \
         'Lt', and 'b' is one, so 'b' is compiled at 'Box<String>'",
    );
    assert_compiler_error(&program, "'b' is called through 'Lt' here");
}

#[test]
fn a_conversion_is_accepted_when_nothing_calls_the_invalid_method_through_the_trait() {
    assert_heap_guard_output(
        &with(
            CALLED_ELSEWHERE,
            r#"
fn main()
    let o Lt = Other(k: 1)
    let b = Box<String>(v: "x" + "y")
    println(f"{o.a()} {only_a(b)}")
"#,
        ),
        "true true",
    );
}
