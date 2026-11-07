// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method on a class, trait or enum may declare its own type parameters,
//! `fn m<T>(x T) T`, which are in scope for its signature and its body.
//!
//! A method's parameter may not reuse a name its owner's parameters already
//! take: inside the method the name would mean two types at once.

use super::utils::*;

/// A class declaring a generic method nothing calls compiles and runs.
#[test]
fn a_class_declares_a_generic_method() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "class Box\n",
            "    var v int\n",
            "    fn init(v int)\n",
            "        self.v = v\n",
            "    fn same<T>(x T) T\n",
            "        return x\n",
            "    fn pair<A, B>(a A, b B) (A, B)\n",
            "        return (a, b)\n",
            "\n",
            "fn main()\n",
            "    let b = Box(3)\n",
            "    println(f\"{b.v}\")\n",
        ),
        "3",
    );
}

/// A generic method on a generic class sees both the class's parameters and
/// its own.
#[test]
fn a_generic_class_declares_a_generic_method() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "class Holder<K>\n",
            "    var k K\n",
            "    fn init(k K)\n",
            "        self.k = k\n",
            "    fn with<T>(x T) (K, T)\n",
            "        return (self.k, x)\n",
            "\n",
            "fn main()\n",
            "    let h = Holder<int>(4)\n",
            "    println(f\"{h.k}\")\n",
        ),
        "4",
    );
}

/// A trait may require a generic method and supply a generic default.
#[test]
fn a_trait_declares_generic_methods() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "trait Gen\n",
            "    fn pick<T>(x T) T\n",
            "    fn keep<T>(x T) T\n",
            "        return x\n",
            "\n",
            "fn main()\n",
            "    println(\"ok\")\n",
        ),
        "ok",
    );
}

/// An enum declaring a generic method nothing calls compiles and runs.
#[test]
fn an_enum_declares_a_generic_method() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "enum Shade\n",
            "    Light\n",
            "    Dark\n",
            "    fn keep<T>(x T) T\n",
            "        return x\n",
            "\n",
            "fn main()\n",
            "    let s = Shade.Dark\n",
            "    println(\"ok\")\n",
        ),
        "ok",
    );
}

/// A bound on a method's own parameter names a trait in scope.
#[test]
fn a_method_type_parameter_takes_a_bound() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "class Box\n",
            "    fn larger<T implements Comparable>(a T, b T) T\n",
            "        if a.compare(b) > 0: return a\n",
            "        return b\n",
            "\n",
            "fn main()\n",
            "    let b = Box()\n",
            "    println(\"ok\")\n",
        ),
        "ok",
    );
}

/// A class method's parameter named after its class's parameter is refused.
#[test]
fn a_class_method_parameter_may_not_reuse_the_class_parameter() {
    assert_compiler_error(
        concat!(
            "class Box<T>\n",
            "    fn m<T>(x T) T\n",
            "        return x\n",
            "\n",
            "fn main()\n",
            "    let b = Box<int>()\n",
        ),
        "type parameter `T` hides the class's type parameter `T`",
    );
}

/// A trait method's parameter named after its trait's parameter is refused.
#[test]
fn a_trait_method_parameter_may_not_reuse_the_trait_parameter() {
    assert_compiler_error(
        concat!(
            "trait Gen<T>\n",
            "    fn m<T>(x T) T\n",
            "\n",
            "fn main()\n",
            "    println(\"ok\")\n",
        ),
        "type parameter `T` hides the trait's type parameter `T`",
    );
}

/// An enum method's parameter named after its enum's parameter is refused.
#[test]
fn an_enum_method_parameter_may_not_reuse_the_enum_parameter() {
    assert_compiler_error(
        concat!(
            "enum Slot<T>\n",
            "    Empty\n",
            "    Full(T)\n",
            "    fn m<T>(x T) T\n",
            "        return x\n",
            "\n",
            "fn main()\n",
            "    println(\"ok\")\n",
        ),
        "type parameter `T` hides the enum's type parameter `T`",
    );
}

/// The MIR `miri build --emit mir` writes for `program`.
fn emitted_mir(program: &str) -> String {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("prog.mi");
    std::fs::write(&source, program).unwrap();
    let out = dir.path().join("prog");
    crate::utils::miri_cmd()
        .arg("build")
        .arg(&source)
        .arg("--out")
        .arg(&out)
        .args(["--emit", "mir"])
        .assert()
        .success();
    std::fs::read_to_string(dir.path().join("prog.mir")).unwrap()
}

/// A generic method is given no body shared by every type argument: one
/// would be laid out for a placeholder no call settles. Neither is a generic
/// method a class receives from its base class or from a trait default, nor
/// one of a generic class at an instantiation of the class: a call names the
/// copy at its own type argument.
#[test]
fn a_generic_method_compiles_no_shared_body() {
    let mir = emitted_mir(concat!(
        "use system.io\n",
        "\n",
        "trait Gen\n",
        "    fn keep_default<T>(x T) T\n",
        "        return x\n",
        "\n",
        "abstract class Base\n",
        "    fn keep_inherited<T>(x T) T\n",
        "        return x\n",
        "\n",
        "class Impl extends Base implements Gen\n",
        "    var v int\n",
        "    fn keep_own<T>(x T) T\n",
        "        return x\n",
        "\n",
        "enum Shade\n",
        "    Light\n",
        "    fn keep_variant<T>(x T) T\n",
        "        return x\n",
        "\n",
        "class Holder<K>\n",
        "    var k K\n",
        "    fn init(k K)\n",
        "        self.k = k\n",
        "    fn keep_instance<T>(x T) T\n",
        "        return x\n",
        "\n",
        "fn main()\n",
        "    let b = Impl(v: 3)\n",
        "    let h = Holder<int>(4)\n",
        "    println(f\"{b.v} {h.keep_instance(h.k)}\")\n",
    ));
    // The call compiles the copy at its own argument, `keep_instance$int`.
    let shared: Vec<&str> = mir
        .lines()
        .filter(|line| line.starts_with("fn ") && line.contains(".keep_"))
        .filter(|line| !line.contains(".keep_instance$int"))
        .collect();
    assert!(
        shared.is_empty(),
        "shared generic method bodies: {shared:?}"
    );
}

/// A class with a generic method `same` and a bounded one `larger`, and a
/// class `Plain` meeting no bound.
const BOX: &str = concat!(
    "class Plain\n",
    "    var n int\n",
    "\n",
    "class Box\n",
    "    fn same<T>(x T) T\n",
    "        return x\n",
    "    fn larger<T implements Comparable>(a T, b T) T\n",
    "        if a.compare(b) > 0: return a\n",
    "        return b\n",
    "\n",
);

/// `BOX` followed by a `main` running `body`.
fn with_box(body: &str) -> String {
    format!("{BOX}fn main()\n    let b = Box()\n{body}")
}

/// A call to a generic method binds the method's parameter from its
/// argument, so the result has the argument's type.
#[test]
fn a_generic_method_call_binds_its_parameter_from_the_argument() {
    assert_runs_with_output(
        &format!(
            "use system.io\n\n{}",
            with_box(concat!(
                "    let n int = b.same(5)\n",
                "    let s String = b.same(\"a\")\n",
                "    println(f\"{n} {s}\")\n",
            ))
        ),
        "5 a",
    );
}

/// The bound type is the argument's, not a placeholder any type matches.
#[test]
fn a_generic_method_call_result_has_the_argument_type() {
    assert_compiler_error(
        &with_box("    let s String = b.same(5)\n"),
        "Type mismatch for variable 's': expected String, got int",
    );
}

/// A generic method of a generic class reads the class's parameter at the
/// receiver's argument and binds its own from the call.
#[test]
fn a_generic_method_of_a_generic_class_binds_both_parameters() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "class Holder<K>\n",
            "    var k K\n",
            "    fn init(k K)\n",
            "        self.k = k\n",
            "    fn with<T>(x T) (K, T)\n",
            "        return (self.k, x)\n",
            "\n",
            "fn main()\n",
            "    let h = Holder<int>(4)\n",
            "    let t (int, String) = h.with(\"a\")\n",
            "    println(f\"{t.0} {t.1}\")\n",
        ),
        "4 a",
    );
}

/// An enum's generic method binds its parameter at the call.
#[test]
fn an_enum_generic_method_call_binds_its_parameter() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "enum Shade\n",
            "    Light\n",
            "    fn keep<T>(x T) T\n",
            "        return x\n",
            "\n",
            "fn main()\n",
            "    let s = Shade.Light\n",
            "    let n int = s.keep(5)\n",
            "    println(f\"{n}\")\n",
        ),
        "5",
    );
}

/// Type arguments written on a method call bind its parameters, and the call
/// runs the copy compiled at them.
#[test]
fn explicit_type_arguments_bind_a_generic_method() {
    assert_runs_with_output(
        &format!(
            "use system.io\n\n{}",
            with_box(concat!(
                "    let n int = b.same<int>(5)\n",
                "    let s = b.same<String>(\"ab\".to_upper())\n",
                "    println(f\"{n} {s}\")\n",
            ))
        ),
        "5 AB",
    );
}

/// An argument the written type argument does not admit is refused.
#[test]
fn an_argument_the_explicit_type_argument_refuses_is_refused() {
    assert_compiler_error(
        &with_box("    let n = b.same<String>(5)\n"),
        "Type mismatch for argument 'x': expected String, got int",
    );
}

/// A method call writing more type arguments than the method declares is
/// refused.
#[test]
fn a_method_call_writing_too_many_type_arguments_is_refused() {
    assert_compiler_error(
        &with_box("    let n = b.same<int, int>(5)\n"),
        "'same' expects 1 type argument, got 2",
    );
}

/// A generic method read as a value is refused, as any method is: only a
/// call binds its type parameters.
#[test]
fn a_generic_method_read_as_a_value_is_refused() {
    assert_compiler_error(
        &with_box("    let f = b.same\n"),
        "the method `same` is read as a value",
    );
}

/// A generic method read as a value with its type arguments written out is
/// refused too.
#[test]
fn a_generic_method_read_with_type_arguments_as_a_value_is_refused() {
    assert_compiler_error(
        &with_box("    let f = b.same<int>\n"),
        "the method `same` is read as a value",
    );
}

/// A call binding a bounded method parameter to a type meeting no bound is
/// refused.
#[test]
fn a_generic_method_bound_the_argument_does_not_meet_is_refused() {
    assert_compiler_error(
        &with_box("    let p = b.larger(Plain(n: 1), Plain(n: 2))\n"),
        "does not satisfy constraint implements Comparable",
    );
}

/// A call meeting the bound is accepted and runs the bound's method.
#[test]
fn a_generic_method_bound_the_argument_meets_is_accepted() {
    assert_runs_with_output(
        &format!(
            "use system.io\n\n{}",
            with_box("    let n int = b.larger(3, 4)\n    println(f\"{n}\")\n")
        ),
        "4",
    );
}

/// A body hashing its unbounded method parameter asks each call for a type
/// with a hash, and a call passing a type that writes its own `equals`
/// without one is refused.
#[test]
fn a_call_pinning_a_hashed_method_parameter_to_a_type_without_hash_is_refused() {
    assert_compiler_error(
        concat!(
            "use system.collections.set\n",
            "\n",
            "class Odd\n",
            "    var n int\n",
            "    public fn equals(other Odd) bool\n",
            "        return other.n == self.n\n",
            "\n",
            "class Box\n",
            "    fn unique<T>(x T) int\n",
            "        let s = Set<T>()\n",
            "        s.add(x)\n",
            "        return s.length()\n",
            "\n",
            "fn main()\n",
            "    let b = Box()\n",
            "    let n = b.unique(Odd(n: 1))\n",
        ),
        "'unique' hashes its 'T' parameter",
    );
}

/// A trait declaring a generic method, a class implementing it, and a
/// function taking the trait at a type parameter.
const GEN: &str = concat!(
    "trait Gen\n",
    "    fn pick<T>(x T) T\n",
    "\n",
    "class Impl implements Gen\n",
    "    public fn pick<T>(x T) T\n",
    "        return x\n",
    "\n",
    "fn through<G implements Gen>(g G) int\n",
    "    return g.pick(5)\n",
    "\n",
);

/// A generic method called through a type parameter bounded by its trait is
/// compiled for each type the parameter is pinned to, and runs.
#[test]
fn a_generic_trait_method_through_a_type_parameter_returns_its_argument() {
    assert_runs_with_output(
        &format!(
            "use system.io\n\n{GEN}fn main()\n    let n int = through(Impl())\n    \
             println(f\"{{n}}\")\n"
        ),
        "5",
    );
}

/// A generic method called through a value of the trait type is refused: a
/// trait value is called through its vtable, and no vtable slot can hold a
/// body for every type argument.
#[test]
fn a_generic_trait_method_through_a_trait_value_is_refused() {
    assert_compiler_error(
        &format!("{GEN}fn main()\n    let g Gen = Impl()\n    let n = g.pick(5)\n"),
        "MER_TYP_087",
    );
}

/// The refusal says how to take the value instead.
#[test]
fn a_generic_trait_method_through_a_trait_value_names_the_repair() {
    assert_compiler_error(
        &format!("{GEN}fn main()\n    let g Gen = Impl()\n    let n = g.pick(5)\n"),
        "take the value as `<G implements Gen>` so each call is compiled for its type",
    );
}

/// A trait default calling a generic method on `self` is compiled once per
/// implementing class, so the call is accepted and runs that class's method.
#[test]
fn a_generic_trait_method_called_on_self_in_a_default_runs() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "trait Gen\n",
            "    fn pick<T>(x T) T\n",
            "    fn twice() int\n",
            "        return self.pick(2) + self.pick(3)\n",
            "\n",
            "class Impl implements Gen\n",
            "    public fn pick<T>(x T) T\n",
            "        return x\n",
            "\n",
            "fn main()\n",
            "    println(f\"{Impl().twice()}\")\n",
        ),
        "5",
    );
}

/// A generic method runs the copy compiled for each type a call binds: an
/// integer and a string built at run time, so an address read as a number
/// or an unretained string cannot pass for the right answer.
#[test]
fn a_generic_method_runs_at_each_type_it_is_called_at() {
    assert_runs_with_output(
        &format!(
            "use system.io\n\n{}",
            with_box(concat!(
                "    println(f\"{b.same(5)}\")\n",
                "    println(b.same(\"ab\".to_upper()))\n",
            ))
        ),
        "5\nAB",
    );
}

/// A generic class with a generic method mapping its elements, read at
/// managed and unmanaged element and result types.
const BAG: &str = concat!(
    "use system.io\n",
    "use system.collections.list\n",
    "\n",
    "class Bag<E>\n",
    "    var items [E]\n",
    "    fn init(items [E])\n",
    "        self.items = items\n",
    "    fn map<U>(f fn(E) U) [U]\n",
    "        var out = List<U>()\n",
    "        for x in self.items\n",
    "            out.push(f(x))\n",
    "        return out\n",
    "\n",
);

/// A generic method of a generic class reads the class's parameter at the
/// receiver's argument and its own at the call's.
#[test]
fn a_generic_method_of_a_generic_class_maps_its_elements() {
    assert_runs_with_output(
        &format!(
            "{BAG}{}",
            concat!(
                "fn main()\n",
                "    let ints = Bag<int>(List([1, 2]))\n",
                "    for s in ints.map(fn(x int) String: f\"<{x}>\")\n",
                "        println(s)\n",
                "    let words = Bag<String>(List([\"ab\".to_upper(), \"c\".to_upper()]))\n",
                "    for n in words.map(fn(x String) int: x.length())\n",
                "        println(f\"{n}\")\n",
                "    for w in words.map(fn(x String) String: x + \"!\")\n",
                "        println(w)\n",
                "    for n in ints.map(fn(x int) int: x * 10)\n",
                "        println(f\"{n}\")\n",
            )
        ),
        "<1>\n<2>\n2\n1\nAB!\nC!\n10\n20",
    );
}

/// A bounded method parameter calls the bound's method at the bound type.
#[test]
fn a_bounded_generic_method_calls_its_bound() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "class Box\n",
            "    fn code<T implements Hashable>(x T) int\n",
            "        return x.hash()\n",
            "\n",
            "fn main()\n",
            "    let b = Box()\n",
            "    let s = \"ab\".to_upper()\n",
            "    let n = 41\n",
            "    println(f\"{b.code(s) == s.hash()} {b.code(n) == n.hash()}\")\n",
        ),
        "true true",
    );
}

/// An enum's generic method runs at each type its calls bind, on a plain
/// enum and on a generic one.
#[test]
fn an_enum_generic_method_runs() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "enum Side\n",
            "    Left\n",
            "    Right\n",
            "    fn pick<T>(a T, b T) T\n",
            "        match self\n",
            "            Side.Left: return a\n",
            "            Side.Right: return b\n",
            "\n",
            "enum Slot<V>\n",
            "    Full(V)\n",
            "    fn with<T>(x T) T\n",
            "        return x\n",
            "\n",
            "fn main()\n",
            "    let l = Side.Left\n",
            "    let r = Side.Right\n",
            "    println(f\"{l.pick(1, 2)} {r.pick(1, 2)}\")\n",
            "    println(r.pick(\"a\".to_upper(), \"b\".to_upper()))\n",
            "    let s Slot<String> = Slot.Full(\"v\".to_upper())\n",
            "    println(s.with(\"w\".to_upper()))\n",
        ),
        "1 2\nB\nW",
    );
}

/// A base class whose generic method its subclasses inherit, called on the
/// subclass and through a binding typed as the base.
const INHERITED: &str = concat!(
    "use system.io\n",
    "\n",
    "abstract class Base\n",
    "    fn same<T>(x T) T\n",
    "        return x\n",
    "    fn name() String\n",
    "        return \"base\"\n",
    "\n",
    "class Child extends Base\n",
    "    var n int\n",
    "    fn name() String\n",
    "        return \"child\"\n",
    "\n",
);

/// A subclass runs the generic method its base class declares.
#[test]
fn an_inherited_generic_method_runs_on_the_subclass() {
    assert_runs_with_output(
        &format!(
            "{INHERITED}{}",
            concat!(
                "fn main()\n",
                "    let c = Child(n: 1)\n",
                "    println(c.same(\"x\".to_upper()))\n",
                "    println(f\"{c.same(7)}\")\n",
            )
        ),
        "X\n7",
    );
}

/// A receiver typed as the base class runs the generic method it does not
/// override, called directly since no vtable slot holds a generic method.
#[test]
fn a_base_typed_receiver_runs_a_generic_method() {
    assert_runs_with_output(
        &format!(
            "{INHERITED}{}",
            concat!(
                "fn main()\n",
                "    let b Base = Child(n: 1)\n",
                "    println(b.same(\"y\".to_upper()))\n",
                "    println(b.name())\n",
            )
        ),
        "Y\nchild",
    );
}

/// A generic base class's generic method, inherited by a subclass pinning
/// the base's parameter, reads both.
#[test]
fn an_inherited_generic_method_of_a_generic_base_runs() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "abstract class Base<K>\n",
            "    var k K\n",
            "    fn with<T>(x T) (K, T)\n",
            "        return (self.k, x)\n",
            "\n",
            "class Child extends Base<String>\n",
            "    fn init(k String)\n",
            "        self.k = k\n",
            "\n",
            "fn main()\n",
            "    let c = Child(\"k\".to_upper())\n",
            "    let p = c.with(3)\n",
            "    let q = c.with(\"s\".to_upper())\n",
            "    println(f\"{p.0}{p.1} {q.0}{q.1}\")\n",
        ),
        "K3 KS",
    );
}

/// A generic method called through a type parameter bounded by its trait
/// runs each implementing class's copy at each type argument.
#[test]
fn a_generic_trait_method_through_a_type_parameter_runs() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "trait Gen\n",
            "    fn pick<T>(a T, b T) T\n",
            "\n",
            "class First implements Gen\n",
            "    public fn pick<T>(a T, b T) T\n",
            "        return a\n",
            "\n",
            "class Second implements Gen\n",
            "    public fn pick<T>(a T, b T) T\n",
            "        return b\n",
            "\n",
            "fn use_it<G implements Gen>(g G) String\n",
            "    let n = g.pick(1, 2)\n",
            "    let s = g.pick(\"x\".to_upper(), \"y\".to_upper())\n",
            "    return f\"{n}{s}\"\n",
            "\n",
            "fn main()\n",
            "    println(use_it(First()))\n",
            "    println(use_it(Second()))\n",
        ),
        "1X\n2Y",
    );
}

/// A trait's generic default runs on each implementing class at each type
/// argument, and a default calling a generic method on `self` reaches the
/// class's own copy.
#[test]
fn a_generic_trait_default_runs_on_an_implementing_class() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "trait Gen\n",
            "    fn pick<T>(a T, b T) T\n",
            "    fn keep<T>(x T) T\n",
            "        return self.pick(x, x)\n",
            "    fn twice() int\n",
            "        return self.pick(2, 9) + self.pick(3, 9)\n",
            "\n",
            "class First implements Gen\n",
            "    var n int\n",
            "    public fn pick<T>(a T, b T) T\n",
            "        return a\n",
            "\n",
            "fn main()\n",
            "    let f = First(n: 1)\n",
            "    println(f.keep(\"k\".to_upper()))\n",
            "    println(f\"{f.keep(4)} {f.twice()}\")\n",
        ),
        "K\n4 5",
    );
}

/// A generic method calling a generic function, and a generic function
/// calling a generic method, each at the type it was itself reached at.
#[test]
fn generic_methods_and_generic_functions_call_each_other() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "fn ident<T>(x T) T\n",
            "    return x\n",
            "\n",
            "class Box\n",
            "    fn through<T>(x T) T\n",
            "        return ident(x)\n",
            "\n",
            "fn via_box<T>(x T) T\n",
            "    let b = Box()\n",
            "    return b.through(x)\n",
            "\n",
            "fn main()\n",
            "    println(via_box(\"v\".to_upper()))\n",
            "    println(f\"{via_box(8)}\")\n",
            "    println(Box().through(\"t\".to_upper()))\n",
        ),
        "V\n8\nT",
    );
}

/// A generic method calling itself at an ever deeper type needs a new copy
/// per call, and is refused as polymorphic recursion.
#[test]
fn a_generic_method_recursing_at_a_growing_type_is_refused() {
    assert_build_error(
        concat!(
            "use system.io\n",
            "use system.collections.list\n",
            "\n",
            "class Box\n",
            "    fn grow<T>(x T, n int) int\n",
            "        if n == 0\n",
            "            return 0\n",
            "        return self.grow(List([x]), n - 1)\n",
            "\n",
            "fn main()\n",
            "    println(f\"{Box().grow(1, 3)}\")\n",
        ),
        "MER_MIR_016",
    );
}

/// A generic method calling itself at an ever deeper tuple builds no class
/// instance on the way, and is refused at the method's own declaration.
#[test]
fn a_generic_method_recursing_at_a_growing_tuple_is_refused() {
    assert_build_error(
        concat!(
            "use system.io\n",
            "\n",
            "class Box\n",
            "    fn grow<T>(x T, n int) int\n",
            "        if n == 0\n",
            "            return 0\n",
            "        return self.grow((x, 1), n - 1)\n",
            "\n",
            "fn main()\n",
            "    println(f\"{Box().grow(1, 3)}\")\n",
        ),
        "instantiating `grow<((…, int), int)>` nests its type argument",
    );
}

/// A generic method mapping strings to strings releases every string it
/// builds and the list it returns, once dropped.
#[test]
fn a_generic_method_mapping_strings_releases_them() {
    assert_heap_guard_output(
        &format!(
            "{BAG}{}",
            concat!(
                "fn main()\n",
                "    let words = Bag<String>(List([\"ab\".to_upper(), \"c\".to_upper()]))\n",
                "    let out = words.map(fn(x String) String: x + x)\n",
                "    println(f\"{out.length()}\")\n",
            )
        ),
        "2",
    );
}

/// A generic method returning its string argument hands the caller its own
/// reference, released once.
#[test]
fn a_generic_method_returning_its_string_argument_releases_it_once() {
    assert_heap_guard_output(
        &format!(
            "use system.io\n\n{}",
            with_box(concat!(
                "    let s = \"ab\".to_upper()\n",
                "    let t = b.same(s)\n",
                "    println(t)\n",
            ))
        ),
        "AB",
    );
}

/// A generic method writing a string through an `out` parameter of its own
/// type parameter releases the string it overwrites.
#[test]
fn a_generic_method_out_parameter_releases_what_it_overwrites() {
    assert_heap_guard_output(
        concat!(
            "use system.io\n",
            "\n",
            "class Box\n",
            "    fn put<T>(x out T, v T)\n",
            "        x = v\n",
            "\n",
            "fn main()\n",
            "    var s = \"a\".to_upper()\n",
            "    Box().put(s, \"b\".to_upper())\n",
            "    println(s)\n",
        ),
        "B",
    );
}

/// A generic method building an instance of a generic class at its own
/// parameter releases it.
#[test]
fn a_generic_method_building_a_generic_instance_releases_it() {
    assert_heap_guard_output(
        concat!(
            "use system.io\n",
            "\n",
            "class Cell<V>\n",
            "    var v V\n",
            "    fn init(v V)\n",
            "        self.v = v\n",
            "\n",
            "class Maker\n",
            "    fn make<U>(x U) U\n",
            "        let c = Cell<U>(x)\n",
            "        return c.v\n",
            "\n",
            "fn main()\n",
            "    let m = Maker()\n",
            "    println(m.make(\"m\".to_upper()))\n",
            "    println(f\"{m.make(6)}\")\n",
        ),
        "M\n6",
    );
}

/// A generic method returning early releases the string it no longer holds.
#[test]
fn a_generic_method_returning_early_releases_its_string() {
    assert_heap_guard_output(
        concat!(
            "use system.io\n",
            "\n",
            "class Box\n",
            "    fn first<T>(a T, b T, flag bool) T\n",
            "        let kept = b\n",
            "        if flag\n",
            "            return a\n",
            "        return kept\n",
            "\n",
            "fn main()\n",
            "    let b = Box()\n",
            "    println(b.first(\"p\".to_upper(), \"q\".to_upper(), true))\n",
            "    println(b.first(\"p\".to_upper(), \"q\".to_upper(), false))\n",
        ),
        "P\nQ",
    );
}

/// A static generic method binds its parameter from the call's argument and
/// runs the copy compiled at it, on a class and on an enum.
#[test]
fn a_static_generic_method_runs_at_each_type_it_is_called_at() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "\n",
            "class Box\n",
            "    static fn make<T>(x T) T\n",
            "        return x\n",
            "\n",
            "enum Shade\n",
            "    Light\n",
            "    static fn keep<T>(x T) T\n",
            "        return x\n",
            "\n",
            "fn main()\n",
            "    println(Box.make(\"s\".to_upper()))\n",
            "    let n int = Box.make(5)\n",
            "    let m = Box.make<int>(6)\n",
            "    println(f\"{n} {m} {Shade.keep(7)}\")\n",
            "    println(Shade.keep(\"e\".to_upper()))\n",
        ),
        "S\n5 6 7\nE",
    );
}

/// A static generic method's result has the type its call binds.
#[test]
fn a_static_generic_method_result_has_the_argument_type() {
    assert_compiler_error(
        concat!(
            "class Box\n",
            "    static fn make<T>(x T) T\n",
            "        return x\n",
            "\n",
            "fn main()\n",
            "    let s String = Box.make(5)\n",
        ),
        "Type mismatch for variable 's': expected String, got int",
    );
}

/// A static generic method returning its string argument hands the caller
/// its own reference, released once.
#[test]
fn a_static_generic_method_returning_its_string_releases_it_once() {
    assert_heap_guard_output(
        concat!(
            "use system.io\n",
            "\n",
            "class Box\n",
            "    static fn make<T>(x T) T\n",
            "        return x\n",
            "\n",
            "fn main()\n",
            "    let s = \"ab\".to_upper()\n",
            "    let t = Box.make(s)\n",
            "    println(t)\n",
            "    println(Box.make(\"cd\".to_upper()))\n",
        ),
        "AB\nCD",
    );
}

/// A trait whose generic method a function reaches through a type parameter
/// bounded by it, called with a value of the trait type.
const PICKER: &str = concat!(
    "use system.io\n",
    "\n",
    "trait Picker\n",
    "    fn pick<T>(a T, b T) T\n",
    "\n",
    "class First implements Picker\n",
    "    fn pick<U>(a U, _b U) U\n",
    "        return a\n",
    "\n",
);

/// A call binding a type parameter to a trait type is refused when the body
/// it binds calls a generic method through that parameter: the copy would be
/// compiled for the trait, which has no body for every type argument.
#[test]
fn a_trait_type_bound_to_a_parameter_calling_a_generic_method_is_refused() {
    let source = format!(
        "{PICKER}fn run<P implements Picker>(p P) String\n    \
         return p.pick<String>(\"a\", \"b\")\n\n\
         fn main()\n    let x Picker = First()\n    println(run(x))\n"
    );
    assert_compiler_error(&source, "MER_TYP_087");
    assert_compiler_error(
        &source,
        "take the value as `<G implements Picker>` so each call is compiled for its type",
    );
}

/// A generic class bound to a trait type is refused the same way when one of
/// its methods calls a generic method through the bound parameter.
#[test]
fn a_trait_type_bound_to_a_class_parameter_calling_a_generic_method_is_refused() {
    let source = format!(
        "{PICKER}class W<P implements Picker>\n    var p P\n    fn init(p P)\n        \
         self.p = p\n    fn go() String\n        return self.p.pick<String>(\"a\", \"b\")\n\n\
         fn main()\n    let x Picker = First()\n    let w = W<Picker>(x)\n    println(w.go())\n"
    );
    assert_compiler_error(&source, "MER_TYP_087");
}

/// A class bound to the same parameter runs the class's own copy.
#[test]
fn a_class_bound_to_a_parameter_calling_a_generic_method_runs() {
    assert_runs_with_output(
        &format!(
            "{PICKER}fn run<P implements Picker>(p P) String\n    \
             return p.pick<String>(\"a\", \"b\")\n\n\
             fn main()\n    println(run(First()))\n"
        ),
        "a",
    );
}

/// A generic method of a generic class runs through two levels of generic
/// parents, at the arguments each `extends` clause maps the receiver's to.
#[test]
fn a_generic_method_of_a_generic_grandparent_runs() {
    assert_heap_guard_output(
        concat!(
            "use system.io\n",
            "\n",
            "class Base<K>\n",
            "    var k K\n",
            "    fn init(k K)\n",
            "        self.k = k\n",
            "    fn with<T>(x T) (K, T)\n",
            "        return (self.k, x)\n",
            "\n",
            "class Mid<V> extends Base<V>\n",
            "    fn init(v V)\n",
            "        super.init(v)\n",
            "\n",
            "class Leaf extends Mid<String>\n",
            "    fn init()\n",
            "        super.init(\"leaf\")\n",
            "\n",
            "fn main()\n",
            "    let l = Leaf()\n",
            "    let p = l.with(3)\n",
            "    println(f\"{p.0} {p.1}\")\n",
            "    let m = Mid<int>(7)\n",
            "    let q = m.with(\"z\")\n",
            "    println(f\"{q.0} {q.1}\")\n",
        ),
        "leaf 3\n7 z",
    );
}

/// A trait default calling a generic method on `self`, reached through a
/// type parameter bounded by the trait, runs each class's own copy, the
/// default where the class keeps it and the class's body where it replaces
/// it.
#[test]
fn a_trait_default_calling_a_generic_method_runs_through_a_bound() {
    assert_heap_guard_output(
        concat!(
            "use system.io\n",
            "\n",
            "trait Gen\n",
            "    fn pick<T>(x T) T\n",
            "    fn twice<T>(x T) T\n",
            "        return self.pick(self.pick(x))\n",
            "\n",
            "class I implements Gen\n",
            "    fn pick<T>(y T) T\n",
            "        return y\n",
            "\n",
            "class J implements Gen\n",
            "    var n int\n",
            "    fn init()\n",
            "        self.n = 0\n",
            "    fn pick<T>(y T) T\n",
            "        return y\n",
            "    fn twice<T>(x T) T\n",
            "        return x\n",
            "\n",
            "fn go<G implements Gen>(g G) int\n",
            "    return g.pick(3) + g.twice(4)\n",
            "\n",
            "fn go2<G implements Gen>(g G) String\n",
            "    return g.twice(\"q\")\n",
            "\n",
            "fn main()\n",
            "    println(f\"{go(I())} {go(J())} {go2(I())} {go2(J())}\")\n",
        ),
        "7 7 q q",
    );
}

/// A trait's generic method takes no vtable slot, so the plain methods
/// beside it keep theirs: a trait value dispatches them, and a bounded
/// parameter reaches the generic one.
#[test]
fn a_generic_method_beside_plain_vtable_slots_runs() {
    assert_heap_guard_output(
        concat!(
            "use system.io\n",
            "\n",
            "trait Gen\n",
            "    fn pick<T>(x T) T\n",
            "    fn name() String\n",
            "    fn num() int\n",
            "\n",
            "class A implements Gen\n",
            "    fn pick<T>(x T) T\n",
            "        return x\n",
            "    fn name() String\n",
            "        return \"A\"\n",
            "    fn num() int\n",
            "        return 1\n",
            "\n",
            "class B implements Gen\n",
            "    fn pick<T>(x T) T\n",
            "        return x\n",
            "    fn name() String\n",
            "        return \"B\"\n",
            "    fn num() int\n",
            "        return 2\n",
            "\n",
            "fn show(g Gen) String\n",
            "    return f\"{g.name()}{g.num()}\"\n",
            "\n",
            "fn via_bound<G implements Gen>(g G) String\n",
            "    return f\"{g.pick<String>('p')}{g.pick(9)}\"\n",
            "\n",
            "fn main()\n",
            "    println(f\"{show(A())} {show(B())} {via_bound(A())} {via_bound(B())}\")\n",
        ),
        "A1 B2 p9 p9",
    );
}

/// A `super` call to a generic method of the base runs the base's copy,
/// from a plain method and from a generic one.
#[test]
fn a_super_call_to_a_generic_method_runs() {
    assert_heap_guard_output(
        concat!(
            "use system.io\n",
            "\n",
            "class A\n",
            "    fn id<T>(x T) T\n",
            "        return x\n",
            "    fn plain(x int) int\n",
            "        return x\n",
            "\n",
            "class B extends A\n",
            "    fn plain(x int) int\n",
            "        return super.id(x) + super.plain(x)\n",
            "    fn via_id<T>(x T) T\n",
            "        return super.id(x)\n",
            "\n",
            "fn main()\n",
            "    let b = B()\n",
            "    println(f\"{b.plain(2)} {b.via_id('s')}\")\n",
        ),
        "4 s",
    );
}

/// A closure inside a generic method captures values of the method's type
/// parameter and runs at each type the method is called at.
#[test]
fn a_closure_capturing_a_method_type_parameter_runs() {
    assert_heap_guard_output(
        concat!(
            "use system.io\n",
            "\n",
            "class U\n",
            "    fn apply<T>(x T) T\n",
            "        let f = fn(y T) T: y\n",
            "        return f(x)\n",
            "    fn lam<T>(x T) T\n",
            "        let g = fn() T: self.apply(x)\n",
            "        return g()\n",
            "\n",
            "fn main()\n",
            "    let u = U()\n",
            "    println(f\"{u.lam(5)} {u.lam('t')} {u.lam(1.5)}\")\n",
        ),
        "5 t 1.5",
    );
}

/// A generic method returns a closure, a list, an optional and a recursive
/// result of its type parameter, each at an integer and a string.
#[test]
fn a_generic_method_returns_closures_lists_and_optionals_of_its_parameter() {
    assert_heap_guard_output(
        concat!(
            "use system.io\n",
            "use system.collections.list\n",
            "\n",
            "class U\n",
            "    fn mk<T>(x T) fn() T\n",
            "        return fn() T: x\n",
            "    fn lst<T>(x T, n int) [T]\n",
            "        var l = List<T>()\n",
            "        for i in 0..n\n",
            "            l.push(x)\n",
            "        return l\n",
            "    fn opt<T>(x T, b bool) T?\n",
            "        if b\n",
            "            return x\n",
            "        return None\n",
            "    fn rec<T>(x T, n int) T\n",
            "        if n == 0\n",
            "            return x\n",
            "        return self.rec(x, n - 1)\n",
            "\n",
            "fn main()\n",
            "    let u = U()\n",
            "    let f = u.mk<int>(9)\n",
            "    println(f\"{f()}\")\n",
            "    let g = u.mk(\"hi\")\n",
            "    println(f\"{g()}\")\n",
            "    let l = u.lst(\"a\", 3)\n",
            "    println(f\"{l.length()}\")\n",
            "    let o = u.opt(5, true)\n",
            "    match o\n",
            "        Some(v): println(f\"{v}\")\n",
            "        None: println(\"none\")\n",
            "    let o2 = u.opt(\"x\", false)\n",
            "    match o2\n",
            "        Some(v): println(v)\n",
            "        None: println(\"none\")\n",
            "    println(f\"{u.rec(4, 3)} {u.rec('z', 2)}\")\n",
        ),
        "9\nhi\n3\n5\nnone\n4 z",
    );
}

/// A generic method returns a list and a list of lists of its parameter,
/// and another takes one back.
#[test]
fn a_generic_method_returns_nested_lists_of_its_parameter() {
    assert_heap_guard_output(
        concat!(
            "use system.io\n",
            "use system.collections.list\n",
            "\n",
            "class U\n",
            "    fn wrap<T>(x T) [T]\n",
            "        return List([x, x])\n",
            "    fn len<T>(xs [T]) int\n",
            "        return xs.length()\n",
            "    fn nest<T>(x T) [[T]]\n",
            "        return List([self.wrap(x)])\n",
            "\n",
            "fn main()\n",
            "    let u = U()\n",
            "    println(f\"{u.len(u.wrap(3))} {u.len(u.nest('a'))} {u.nest(2.5)[0][1]}\")\n",
        ),
        "2 1 2.5",
    );
}

/// Two generic methods calling each other at the same type parameter, and
/// one recursing at its own, each compile one copy per type and run.
#[test]
fn mutually_recursive_generic_methods_run() {
    assert_heap_guard_output(
        concat!(
            "use system.io\n",
            "\n",
            "class Q\n",
            "    fn a<T>(x T, n int) T\n",
            "        if n == 0\n",
            "            return x\n",
            "        return self.b(x, n - 1)\n",
            "    fn b<U>(x U, n int) U\n",
            "        return self.a(x, n)\n",
            "\n",
            "fn main()\n",
            "    println(f\"{Q().a(1, 3)} {Q().a('s', 3)}\")\n",
        ),
        "1 s",
    );
}

/// A generic method writing its parameter's type through an `out`
/// parameter writes the caller's variable.
#[test]
fn a_generic_method_writes_an_out_parameter_of_its_type() {
    assert_heap_guard_output(
        concat!(
            "use system.io\n",
            "\n",
            "class U\n",
            "    fn put<T>(x T, r out T)\n",
            "        r = x\n",
            "\n",
            "fn main()\n",
            "    let u = U()\n",
            "    var q = 0\n",
            "    u.put(5, q)\n",
            "    println(f\"{q}\")\n",
        ),
        "5",
    );
}

/// A generic method passing and swapping a resource hands each instance on
/// once: every one is dropped exactly once, at the end of `main`.
#[test]
fn a_generic_method_passing_a_resource_drops_it_once() {
    assert_heap_guard_output(
        concat!(
            "use system.io\n",
            "\n",
            "class R\n",
            "    var n int\n",
            "    fn init(n int)\n",
            "        self.n = n\n",
            "    fn drop()\n",
            "        println(f\"drop {self.n}\")\n",
            "\n",
            "class U\n",
            "    fn pass<T>(x T) T\n",
            "        return x\n",
            "    fn swap<T>(a T, b T) (T, T)\n",
            "        return (b, a)\n",
            "\n",
            "fn main()\n",
            "    let u = U()\n",
            "    let r = u.pass(R(1))\n",
            "    let p = u.swap(R(2), R(3))\n",
            "    println(f\"{r.n} {p.0.n}\")\n",
        ),
        "1 3\ndrop 3\ndrop 2\ndrop 1",
    );
}

/// A module's class whose public generic method calls its private one.
const PRIVATE_GENERIC_LIB: &str = concat!(
    "class Lib\n",
    "    private fn hid<T>(x T) T\n",
    "        return x\n",
    "    fn open<T>(x T) T\n",
    "        return self.hid(x)\n",
);

/// A private generic method of another module's class is compiled for the
/// calls its public one makes, at each type the importer passes.
#[test]
fn a_public_generic_method_calling_a_private_one_runs_across_modules() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.lib.{Lib}\n",
                    "\n",
                    "fn main()\n",
                    "    let l = Lib()\n",
                    "    println(f\"{l.open(3)} {l.open(2.5)} {l.open<String>('k')}\")\n",
                ),
            ),
            ("lib.mi", PRIVATE_GENERIC_LIB),
        ],
        "3 2.5 k",
    );
}

/// The private generic method itself is not callable from the importer.
#[test]
fn a_private_generic_method_of_another_module_is_refused() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.lib.{Lib}\n",
                    "\n",
                    "fn main()\n",
                    "    let l = Lib()\n",
                    "    println(f\"{l.hid(3)}\")\n",
                ),
            ),
            ("lib.mi", PRIVATE_GENERIC_LIB),
        ],
        "MER_TYP_035",
    );
}

/// Two generic methods of one name in one class: the later declaration is
/// refused where it is written, even when the only call matches the
/// earlier's arity. The same holds for two non-generic methods of one name.
#[test]
fn a_second_generic_method_of_one_name_is_refused() {
    assert_compiler_error(
        concat!(
            "use system.io\n",
            "\n",
            "class D\n",
            "    fn f<T>(x T) T\n",
            "        return x\n",
            "    fn f<U>(_x U, y U) U\n",
            "        return y\n",
            "\n",
            "fn main()\n",
            "    let d = D()\n",
            "    println(f\"{d.f(1)}\")\n",
        ),
        "Method 'f' is already declared in this class",
    );
}

/// A function handing its parameter on to a body that calls the generic
/// method is refused at the call binding it to the trait, and the refusal
/// says the function reaches the method rather than calls it.
#[test]
fn a_trait_type_bound_to_a_parameter_handed_on_to_a_generic_method_call_says_reaches() {
    let source = format!(
        "{PICKER}fn run<P implements Picker>(p P) String\n    \
         return p.pick<String>(\"a\", \"b\")\n\n\
         fn outer<Q implements Picker>(q Q) String\n    return run(q)\n\n\
         fn main()\n    let x Picker = First()\n    println(outer(x))\n"
    );
    assert_compiler_error(&source, "MER_TYP_087");
    assert_compiler_error(
        &source,
        "`outer` reaches the generic method `pick` through its type parameter `Q`",
    );
}

/// A function calling the generic method itself is still said to call it.
#[test]
fn a_trait_type_bound_to_a_parameter_calling_a_generic_method_says_calls() {
    let source = format!(
        "{PICKER}fn run<P implements Picker>(p P) String\n    \
         return p.pick<String>(\"a\", \"b\")\n\n\
         fn main()\n    let x Picker = First()\n    println(run(x))\n"
    );
    assert_compiler_error(
        &source,
        "`run` calls the generic method `pick` through its type parameter `P`",
    );
}
