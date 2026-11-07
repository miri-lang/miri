// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Miri has no overloading: one class, enum or trait declares a method name
//! once. A second declaration is refused where it is written, naming the
//! first, rather than replacing it.

use super::utils::{assert_compiler_error, assert_runs_with_output};

/// A class declaring two methods of one name, differing in arity.
const CLASS_TWICE: &str = "use system.io

class D
    fn f(x int) int
        return x
    fn f(x int, y int) int
        return y

fn main()
    let d = D()
    println(f\"{d.f(2, 3)}\")
";

#[test]
fn a_class_declaring_a_method_twice_is_refused() {
    assert_compiler_error(CLASS_TWICE, "MER_TYP_070");
    assert_compiler_error(CLASS_TWICE, "Method 'f' is already declared in this class");
}

/// The refusal lands on the second declaration (line 6) and its note points
/// at the first (line 4).
#[test]
fn a_class_method_redeclaration_names_the_first_declaration() {
    assert_compiler_error(CLASS_TWICE, ":6:8\n");
    assert_compiler_error(CLASS_TWICE, "the first declaration of 'f' is here");
    assert_compiler_error(CLASS_TWICE, ":4:8\n");
}

#[test]
fn a_class_declaring_a_generic_method_twice_is_refused() {
    assert_compiler_error(
        "use system.io

class D
    fn f<T>(x T) T
        return x
    fn f<U>(x U, y U) U
        return y

fn main()
    let d = D()
    println(f\"{d.f(2, 3)}\")
",
        "Method 'f' is already declared in this class",
    );
}

#[test]
fn an_enum_declaring_a_method_twice_is_refused() {
    assert_compiler_error(
        "enum Color
    Red
    Green

    fn code() int
        return 1
    fn code(x int) int
        return x

fn main()
    let c = Color.Red
",
        "Method 'code' is already declared in this enum",
    );
}

#[test]
fn an_enum_declaring_a_generic_method_twice_is_refused() {
    assert_compiler_error(
        "enum Color
    Red
    Green

    fn same<T>(x T) T
        return x
    fn same(x int) int
        return x

fn main()
    let c = Color.Red
",
        "Method 'same' is already declared in this enum",
    );
}

/// An enum's static and instance methods share one name space, as a
/// class's do.
#[test]
fn an_enum_declaring_a_static_and_an_instance_method_of_one_name_is_refused() {
    assert_compiler_error(
        "enum Color
    Red
    Green

    fn code() int
        return 1
    static fn code() int
        return 2

fn main()
    let c = Color.Red
",
        "Method 'code' cannot be both instance and static method",
    );
}

#[test]
fn a_trait_declaring_a_method_twice_is_refused() {
    assert_compiler_error(
        "trait Speaker
    fn speak() String
    fn speak(loud bool) String

fn main()
    let n = 1
",
        "Method 'speak' is already declared in this trait",
    );
}

#[test]
fn a_trait_declaring_a_generic_method_twice_is_refused() {
    assert_compiler_error(
        "trait Picker
    fn pick<T>(a T) T
    fn pick<T>(a T, b T) T

fn main()
    let n = 1
",
        "Method 'pick' is already declared in this trait",
    );
}

/// A subclass redeclaring its base's method overrides it: the two
/// declarations belong to different classes.
#[test]
fn a_subclass_overriding_a_base_method_runs() {
    assert_runs_with_output(
        "use system.io

class Base
    fn name() String
        return \"base\"

class Child extends Base
    fn name() String
        return \"child\"

fn main()
    let b Base = Child()
    println(b.name())
",
        "child",
    );
}

/// A class implementing a trait method declares it once, in the class.
#[test]
fn a_class_implementing_a_trait_method_runs() {
    assert_runs_with_output(
        "use system.io

trait Speaker
    fn speak() String
    fn shout() String
        return \"default\"

class Dog implements Speaker
    fn speak() String
        return \"woof\"
    fn shout() String
        return \"WOOF\"

fn main()
    let d = Dog()
    println(f\"{d.speak()} {d.shout()}\")
",
        "woof WOOF",
    );
}
