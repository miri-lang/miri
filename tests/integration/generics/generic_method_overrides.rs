// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic method is compiled once per type argument its calls bind, and
//! every call to one names its copy statically: no vtable slot holds one.
//! A subclass redeclaring a generic method of a class it extends would
//! therefore never run through a receiver typed as the base, so the
//! redeclaration is refused, and so is a generic method a class leaves
//! without a body. A class implementing a trait's generic method matches it
//! by position: its type parameters are read as the trait's.

use super::utils::*;

/// The help every refused override carries.
const OVERRIDE_HELP: &str = "a generic method cannot be overridden; move the part that varies \
                             into a non-generic method that `Dog` overrides, or declare the \
                             type parameter on the class";

/// A base class whose generic method `speak` a subclass `Dog` redeclares
/// with `child`.
fn overriding(child: &str) -> String {
    format!(
        concat!(
            "class Animal\n",
            "    fn speak<T>(x T) String\n",
            "        return \"animal\"\n",
            "\n",
            "class Dog extends Animal\n",
            "{}",
            "\n",
            "fn main()\n",
            "    let b Animal = Dog()\n",
            "    let s = b.speak(1)\n",
        ),
        child
    )
}

/// A non-generic method redeclaring a base's generic method is refused,
/// naming the base class.
#[test]
fn a_method_overriding_a_generic_method_is_refused() {
    assert_compiler_error(
        &overriding("    fn speak(x int) String\n        return \"dog\"\n"),
        "MER_TYP_088",
    );
    assert_compiler_error(
        &overriding("    fn speak(x int) String\n        return \"dog\"\n"),
        "`speak` of `Dog` overrides the generic method `speak` of `Animal`",
    );
}

/// A generic method redeclaring a base's generic method is refused too.
#[test]
fn a_generic_method_overriding_a_generic_method_is_refused() {
    assert_compiler_error(
        &overriding("    fn speak<T>(x T) String\n        return \"dog\"\n"),
        "`speak` of `Dog` overrides the generic method `speak` of `Animal`",
    );
}

/// The refusal says how to keep the part that varies overridable.
#[test]
fn an_overridden_generic_method_names_the_repair() {
    assert_compiler_error(
        &overriding("    fn speak<T>(x T) String\n        return \"dog\"\n"),
        OVERRIDE_HELP,
    );
}

/// The refusal points at the overriding method, not at the class.
#[test]
fn an_overridden_generic_method_is_reported_at_the_override() {
    assert_compiler_error(
        &overriding("    var n int\n    fn speak<T>(x T) String\n        return \"dog\"\n"),
        ":7:5",
    );
}

/// A generic method redeclaring a base's non-generic method is refused.
#[test]
fn a_generic_method_overriding_a_plain_method_is_refused() {
    assert_compiler_error(
        concat!(
            "class Animal\n",
            "    fn speak(x int) String\n",
            "        return \"animal\"\n",
            "\n",
            "class Dog extends Animal\n",
            "    fn speak<T>(x T) String\n",
            "        return \"dog\"\n",
            "\n",
            "fn main()\n",
            "    let d = Dog()\n",
        ),
        "the generic method `speak` of `Dog` overrides `speak` of `Animal`",
    );
}

/// A generic method declared further up the chain is refused at a
/// redeclaration two classes down, naming the class that declares it.
#[test]
fn a_generic_method_of_a_grandparent_is_not_overridden() {
    assert_compiler_error(
        concat!(
            "class Animal\n",
            "    fn speak<T>(x T) String\n",
            "        return \"animal\"\n",
            "\n",
            "class Hound extends Animal\n",
            "    var n int\n",
            "\n",
            "class Dog extends Hound\n",
            "    fn speak<T>(x T) String\n",
            "        return \"dog\"\n",
            "\n",
            "fn main()\n",
            "    let d = Dog(n: 1)\n",
        ),
        "`speak` of `Dog` overrides the generic method `speak` of `Animal`",
    );
}

/// A generic default a base class takes from a trait is the base's own
/// generic method, and a subclass redeclaring it is refused.
#[test]
fn a_generic_default_a_base_class_inherits_is_not_overridden() {
    assert_compiler_error(
        concat!(
            "trait Speaker\n",
            "    fn speak<T>(x T) String\n",
            "        return \"trait\"\n",
            "\n",
            "class Animal implements Speaker\n",
            "    var n int\n",
            "\n",
            "class Dog extends Animal\n",
            "    public fn speak<T>(x T) String\n",
            "        return \"dog\"\n",
            "\n",
            "fn main()\n",
            "    let d = Dog(n: 1)\n",
        ),
        "`speak` of `Dog` overrides the generic method `speak` of `Animal`",
    );
}

/// A class may not leave a generic method without a body: no subclass could
/// supply one. Only a trait declares a generic method without a body.
#[test]
fn an_abstract_generic_method_of_a_class_is_refused() {
    assert_compiler_error(
        concat!(
            "abstract class Animal\n",
            "    fn speak<T>(x T) String\n",
            "\n",
            "fn main()\n",
            "    println(\"ok\")\n",
        ),
        "the generic method `speak` of `Animal` has no body",
    );
}

/// The subclass's other methods still override as before.
#[test]
fn a_class_with_a_generic_method_still_overrides_its_plain_ones() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "class Animal\n",
            "    fn same<T>(x T) T\n",
            "        return x\n",
            "    fn name() String\n",
            "        return \"animal\"\n",
            "\n",
            "class Dog extends Animal\n",
            "    fn name() String\n",
            "        return \"dog\"\n",
            "\n",
            "fn main()\n",
            "    let b Animal = Dog()\n",
            "    println(f\"{b.name()} {b.same(4)}\")\n",
        ),
        "dog 4",
    );
}

/// A trait requiring a generic method `pick`, a class implementing it with
/// `implementation`, and a call through a type parameter bounded by the
/// trait.
fn implementing(trait_method: &str, implementation: &str) -> String {
    format!(
        concat!(
            "use system.io\n",
            "\n",
            "trait Gen\n",
            "{}",
            "\n",
            "class Impl implements Gen\n",
            "{}",
            "\n",
            "fn main()\n",
            "    println(\"ok\")\n",
        ),
        trait_method, implementation
    )
}

/// A class's type parameter is read as the trait's at the same position, so
/// `fn pick<U>(x U) U` implements `fn pick<T>(x T) T`.
#[test]
fn a_generic_trait_method_is_implemented_under_other_parameter_names() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "trait Gen\n",
            "    fn pick<T>(a T, b T) T\n",
            "\n",
            "class Impl implements Gen\n",
            "    public fn pick<U>(a U, b U) U\n",
            "        return b\n",
            "\n",
            "fn through<G implements Gen>(g G) String\n",
            "    return g.pick(\"x\".to_upper(), \"y\".to_upper())\n",
            "\n",
            "fn main()\n",
            "    println(through(Impl()))\n",
            "    println(f\"{Impl().pick(1, 2)}\")\n",
        ),
        "Y\n2",
    );
}

/// The bounds are compared after the renaming, so equal bounds under other
/// names are accepted.
#[test]
fn a_bounded_generic_trait_method_is_implemented_under_other_names() {
    assert_type_checks(&implementing(
        "    fn larger<T implements Comparable>(a T, b T) T\n",
        concat!(
            "    public fn larger<U implements Comparable>(a U, b U) U\n",
            "        if a.compare(b) > 0: return a\n",
            "        return b\n",
        ),
    ));
}

/// An implementation declaring a different number of type parameters is
/// refused, naming the method.
#[test]
fn an_implementation_with_another_parameter_count_is_refused() {
    assert_compiler_error(
        &implementing(
            "    fn pick<T>(x T) T\n",
            "    public fn pick<T, U>(x T) T\n        return x\n",
        ),
        "Method 'pick' in class 'Impl' does not match trait 'Gen' signature: expected fn pick<T>(x: T) -> T",
    );
}

/// An implementation bounding its parameter differently is refused.
#[test]
fn an_implementation_with_another_bound_is_refused() {
    assert_compiler_error(
        &implementing(
            "    fn pick<T implements Comparable>(x T) T\n",
            "    public fn pick<U>(x U) U\n        return x\n",
        ),
        "Method 'pick' in class 'Impl' does not match trait 'Gen' signature",
    );
}

/// An implementation taking a concrete type where the trait takes its type
/// parameter is refused.
#[test]
fn an_implementation_taking_a_concrete_type_is_refused() {
    assert_compiler_error(
        &implementing(
            "    fn pick<T>(x T) T\n",
            "    public fn pick<T>(x int) T\n        return x\n",
        ),
        "Method 'pick' in class 'Impl' does not match trait 'Gen' signature",
    );
}

/// A generic implementation of a non-generic trait method is refused: the
/// trait's method takes a vtable slot, which a generic one cannot fill.
#[test]
fn a_generic_implementation_of_a_plain_trait_method_is_refused() {
    assert_compiler_error(
        &implementing(
            "    fn pick(x int) int\n",
            "    public fn pick<T>(x int) int\n        return x\n",
        ),
        "Method 'pick' in class 'Impl' does not match trait 'Gen' signature: expected fn pick(x: int) -> int",
    );
}

/// A plain implementation of a generic trait method is refused.
#[test]
fn a_plain_implementation_of_a_generic_trait_method_is_refused() {
    assert_compiler_error(
        &implementing(
            "    fn pick<T>(x int) int\n",
            "    public fn pick(x int) int\n        return x\n",
        ),
        "Method 'pick' in class 'Impl' does not match trait 'Gen' signature: expected fn pick<T>(x: int) -> int",
    );
}

/// A class implementing a trait's generic default supplies its own body,
/// which is what runs, called on the class and through a type parameter
/// bounded by the trait.
#[test]
fn a_class_body_replaces_a_generic_trait_default() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "trait Gen\n",
            "    fn tag<T>(x T) String\n",
            "        return \"trait\"\n",
            "\n",
            "class Own implements Gen\n",
            "    public fn tag<T>(x T) String\n",
            "        return \"class\"\n",
            "\n",
            "class Kept implements Gen\n",
            "    var n int\n",
            "\n",
            "fn through<G implements Gen>(g G) String\n",
            "    return g.tag(1)\n",
            "\n",
            "fn main()\n",
            "    let o = Own()\n",
            "    println(f\"{o.tag(1)} {o.tag('s')} {through(o)}\")\n",
            "    let k = Kept(n: 1)\n",
            "    println(f\"{k.tag(1)} {through(k)}\")\n",
        ),
        "class class class\ntrait trait",
    );
}

/// A static method is called through the class that names it, so a subclass
/// declaring a static method named like a base's static generic one
/// overrides nothing, and each call runs its own class's body.
#[test]
fn a_static_generic_method_beside_a_subclass_static_is_accepted() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "class Animal\n",
            "    static fn make<T>(x T) String\n",
            "        return \"animal\"\n",
            "\n",
            "class Dog extends Animal\n",
            "    static fn make<T>(x T) String\n",
            "        return \"dog\"\n",
            "\n",
            "fn main()\n",
            "    println(f\"{Animal.make(1)} {Dog.make(1)}\")\n",
        ),
        "animal dog",
    );
}

/// An abstract class implementing a trait leaves no generic method of the
/// trait without a body: a subclass could not supply one, since a call
/// through a receiver typed as the abstract class names its copy.
#[test]
fn an_abstract_class_leaving_a_trait_generic_method_open_is_refused() {
    let source = concat!(
        "use system.io\n",
        "\n",
        "trait Gen\n",
        "    fn pick<T>(a T, b T) T\n",
        "\n",
        "abstract class Base implements Gen\n",
        "    var n int\n",
        "\n",
        "class Child extends Base\n",
        "    fn init()\n",
        "        self.n = 1\n",
        "\n",
        "fn main()\n",
        "    let b Base = Child()\n",
        "    println(f\"{b.pick(1, 2)}\")\n",
    );
    assert_compiler_error(source, "MER_TYP_088");
    assert_compiler_error(
        source,
        "the abstract class `Base` leaves the generic method `pick` of the trait `Gen` without a body",
    );
    assert_compiler_error(
        source,
        "give `pick` a body in `Base`; a generic method is compiled per call, so a subclass \
         cannot supply it later",
    );
}

/// A trait's generic default an abstract class inherits is a body, compiled
/// per call, and runs through a receiver typed as the abstract class.
#[test]
fn an_abstract_class_inheriting_a_trait_generic_default_runs_it() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "trait Gen\n",
            "    fn keep<T>(x T) T\n",
            "        return x\n",
            "\n",
            "abstract class Base implements Gen\n",
            "    var n int\n",
            "\n",
            "class Child extends Base\n",
            "    fn init()\n",
            "        self.n = 1\n",
            "\n",
            "fn main()\n",
            "    let b Base = Child()\n",
            "    println(f\"{b.keep(4)}\")\n",
            "    println(b.keep(\"k\".to_upper()))\n",
        ),
        "4\nK",
    );
}

/// The help a constructor or a drop hook declaring type parameters carries.
const CONSTRUCTION_HELP: &str = "declare the type parameter on the class";

/// A constructor declaring type parameters of its own is refused: building
/// an instance binds no type arguments for it, so no copy could be named.
#[test]
fn a_constructor_declaring_type_parameters_is_refused() {
    let source = concat!(
        "use system.io\n",
        "\n",
        "class D\n",
        "    var s String\n",
        "    fn init<T>(_x T)\n",
        "        self.s = \"a\"\n",
        "\n",
        "fn main()\n",
        "    let d = D(1)\n",
        "    println(d.s)\n",
    );
    assert_compiler_error(source, "MER_TYP_088");
    assert_compiler_error(source, CONSTRUCTION_HELP);
}

/// A drop hook declaring type parameters of its own is refused: releasing an
/// instance binds no type arguments for it.
#[test]
fn a_drop_hook_declaring_type_parameters_is_refused() {
    let source = concat!(
        "use system.io\n",
        "\n",
        "class D\n",
        "    var s String\n",
        "    fn init()\n",
        "        self.s = \"a\"\n",
        "    fn drop<T>()\n",
        "        println(\"d\")\n",
        "\n",
        "fn main()\n",
        "    let d = D()\n",
        "    println(d.s)\n",
    );
    assert_compiler_error(source, "MER_TYP_088");
    assert_compiler_error(source, CONSTRUCTION_HELP);
}

/// An enum's drop hook declaring type parameters is refused the same way.
#[test]
fn an_enum_drop_hook_declaring_type_parameters_is_refused() {
    assert_compiler_error(
        concat!(
            "use system.io\n",
            "\n",
            "enum E\n",
            "    A\n",
            "    fn drop<T>()\n",
            "        println(\"d\")\n",
            "\n",
            "fn main()\n",
            "    let e = E.A\n",
            "    println(\"x\")\n",
        ),
        "declare the type parameter on the enum",
    );
}
