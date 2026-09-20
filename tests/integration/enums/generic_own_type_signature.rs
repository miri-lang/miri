// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic enum names itself in a method signature the same three ways a
//! generic class does — the bare `Holder`, the written `Holder<T>`, and `Self`
//! — and all three mean the enum at its own parameters, so a call site that
//! substitutes the receiver's type arguments reads them as the receiver's own
//! instantiation.

use super::utils::*;

const SELF_HOLDER: &str = r#"
enum Holder<T>
    One(T)
    Two(T, T)

    fn same_size(other Self) bool
        return self.size() == other.size()

    fn twin() Self
        return self

    fn size() int
        match self
            Holder.One(_): 1
            Holder.Two(_, _): 2
"#;

fn with_self_holder(main: &str) -> String {
    format!("{SELF_HOLDER}\n{main}")
}

#[test]
fn self_parameter_on_a_generic_enum_method_accepts_the_receivers_instantiation() {
    assert_runs_with_output(
        &with_self_holder(
            r#"
fn main()
    let a Holder<int> = Holder.One(7)
    let b Holder<int> = Holder.One(9)
    let c Holder<int> = Holder.Two(1, 2)
    println(f"{a.same_size(b)},{a.same_size(c)}")
"#,
        ),
        "true,false",
    );
}

#[test]
fn self_return_type_on_a_generic_enum_method_is_the_receivers_instantiation() {
    assert_runs_with_output(
        &with_self_holder(
            r#"
fn main()
    let a Holder<int> = Holder.Two(1, 2)
    let t Holder<int> = a.twin()
    println(f"{t.size()},{a.same_size(t)}")
"#,
        ),
        "2,true",
    );
}

#[test]
fn self_parameter_on_a_generic_enum_method_refuses_a_different_instantiation() {
    assert_compiler_error(
        &with_self_holder(
            r#"
fn main()
    let a Holder<int> = Holder.One(7)
    let s Holder<String> = Holder.One("x")
    println(f"{a.same_size(s)}")
"#,
        ),
        "expected Holder<int>, got Holder<String>",
    );
}

#[test]
fn bare_written_and_self_spellings_of_the_own_enum_agree() {
    assert_runs_with_output(
        r#"
enum Holder<T>
    One(T)
    Two(T, T)

    fn same(other Holder) bool
        return self.size() == other.size()

    fn same_written(other Holder<T>) bool
        return self.size() == other.size()

    fn same_self(other Self) bool
        return self.size() == other.size()

    fn size() int
        match self
            Holder.One(_): 1
            Holder.Two(_, _): 2

fn main()
    let a Holder<int> = Holder.One(7)
    let b Holder<int> = Holder.One(9)
    let c Holder<int> = Holder.Two(1, 2)
    println(f"{a.same(b)},{a.same_written(b)},{a.same_self(b)}")
    println(f"{a.same(c)},{a.same_written(c)},{a.same_self(c)}")
"#,
        "true,true,true\nfalse,false,false",
    );
}

#[test]
fn match_self_inside_a_generic_enum_method_reads_the_payload() {
    assert_runs_with_output(
        r#"
enum Holder<T>
    One(T)
    Two(T, T)

    fn first() T
        match self
            Holder.One(v): v
            Holder.Two(v, _): v

fn main()
    let a Holder<int> = Holder.One(7)
    let b Holder<int> = Holder.Two(3, 4)
    println(f"{a.first()},{b.first()}")
"#,
        "7,3",
    );
}

#[test]
fn a_managed_payload_survives_a_self_parameter_call() {
    assert_runs_with_output(
        r#"
enum Holder<T>
    One(T)
    Two(T, T)

    fn wider_than(other Self) bool
        return self.size() > other.size()

    fn size() int
        match self
            Holder.One(_): 1
            Holder.Two(_, _): 2

fn main()
    let a Holder<String> = Holder.One("pear")
    let b Holder<String> = Holder.Two("fig", "plum")
    println(f"{a.wider_than(b)},{b.wider_than(a)}")
"#,
        "false,true",
    );
}

#[test]
fn the_equal_operator_calls_a_self_parameter_equals_on_a_generic_enum() {
    assert_runs_with_output(
        r#"
enum Holder<T>
    One(T)
    Two(T, T)

    fn equals(other Self) bool
        return self.size() == other.size()

    fn size() int
        match self
            Holder.One(_): 1
            Holder.Two(_, _): 2

fn main()
    let a Holder<int> = Holder.One(7)
    let b Holder<int> = Holder.One(9)
    let c Holder<int> = Holder.Two(1, 2)
    println(f"{a == b},{a == c},{a != c}")
"#,
        "true,false,true",
    );
}

#[test]
fn a_static_method_on_a_generic_enum_returns_the_enum_at_its_instantiation() {
    assert_runs_with_output(
        r#"
enum Holder<T>
    One(T)
    Two(T, T)

    static fn pair(a T, b T) Self
        return Holder.Two(a, b)

    fn size() int
        match self
            Holder.One(_): 1
            Holder.Two(_, _): 2

fn main()
    let p Holder<int> = Holder.pair(1, 2)
    println(f"{p.size()}")
"#,
        "2",
    );
}

#[test]
fn a_self_typed_option_return_carries_the_receivers_instantiation() {
    assert_runs_with_output(
        r#"
enum Holder<T>
    One(T)
    Two(T, T)

    fn maybe(flag bool) Self?
        if flag
            return self
        return None

    fn size() int
        match self
            Holder.One(_): 1
            Holder.Two(_, _): 2

fn main()
    let a Holder<int> = Holder.Two(1, 2)
    match a.maybe(true)
        Some(h): println(f"some {h.size()}")
        None: println("none")
    match a.maybe(false)
        Some(h): println(f"some {h.size()}")
        None: println("none")
"#,
        "some 2\nnone",
    );
}

#[test]
fn a_self_parameter_on_a_non_generic_enum_still_works() {
    assert_runs_with_output(
        r#"
enum Flag
    On
    Off

    fn same(other Self) bool
        return self.bit() == other.bit()

    fn bit() int
        match self
            Flag.On: 1
            Flag.Off: 0

fn main()
    println(f"{Flag.On.same(Flag.On)},{Flag.On.same(Flag.Off)}")
"#,
        "true,false",
    );
}

#[test]
fn a_bare_generic_enum_name_inside_another_definition_is_refused() {
    assert_compiler_error(
        r#"
enum Holder<T>
    One(T)
    Two(T, T)

class Reader
    fn takes(other Holder) bool
        return true

fn main()
    let r = Reader()
    let a Holder<int> = Holder.One(7)
    println(f"{r.takes(a)}")
"#,
        "Generic argument count mismatch: expected 1, got 0",
    );
}

#[test]
fn a_generic_enum_with_two_parameters_names_itself_at_both() {
    assert_runs_with_output(
        r#"
enum Either<L, R>
    Left(L)
    Right(R)

    fn same_side(other Self) bool
        return self.side() == other.side()

    fn side() int
        match self
            Either.Left(_): 0
            Either.Right(_): 1

fn main()
    let a Either<int, String> = Either.Left(1)
    let b Either<int, String> = Either.Left(2)
    let c Either<int, String> = Either.Right("x")
    println(f"{a.same_side(b)},{a.same_side(c)}")
"#,
        "true,false",
    );
}

#[test]
fn a_generic_enum_method_returning_a_list_of_its_own_type_links_and_runs() {
    assert_heap_guard_output(
        r#"
use system.collections.list

enum Holder<T>
    One(T)
    Two(T, T)

    fn alone() List<Self>
        var out = List<Self>()
        out.push(self)
        return out

    fn size() int
        match self
            Holder.One(_): 1
            Holder.Two(_, _): 2

fn main()
    let a Holder<String> = Holder.Two("a" + "b", "c" + "d")
    let xs = a.alone()
    println(f"{xs.length()} {xs[0].size()}")
"#,
        "1 2",
    );
}
