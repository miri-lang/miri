// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Calling a generic function with its type arguments written out — `f<int>(x)`
//! — means the same call as `f(x)` with the argument inferred. The spelling
//! parses as a generic type reference wrapping the function's name, so the name
//! has to be recovered before the call is lowered; left wrapped, it reaches the
//! arm that refuses a type used as a value.
//!
//! Writing the arguments out is the only way to reach an instantiation nothing
//! else pins — a function whose parameter list mentions the parameter in no
//! position an argument can be inferred from.

use super::utils::*;

#[test]
fn an_explicit_type_argument_calls_the_same_body_inference_would() {
    assert_runs_with_output(
        r#"
fn ident<T>(a T) T
    return a

fn main()
    let inferred = ident(5)
    let written = ident<int>(5)
    println(f"{inferred} {written}")
"#,
        "5 5",
    );
}

/// The instantiation is one only the written argument pins, and its layout
/// matters: the class holds a managed value, which a body compiled for the
/// bare parameter would neither size nor release as a string.
#[test]
fn an_explicit_type_argument_reaches_an_instantiation_nothing_else_pins() {
    assert_heap_guard_output(
        r#"
class Box<T>
    v T?
    fn put(a T) T
        self.v = a
        return a

fn make<T>() Box<T>
    return Box<T>()

fn main()
    let b = make<String>()
    println(b.put("a" + "b"))
"#,
        "ab",
    );
}

#[test]
fn an_explicit_type_argument_pins_a_returned_trait() {
    assert_heap_guard_output(
        r#"
trait Op<T>
    fn keep(a T) T

class Impl<T> implements Op<T>
    fn keep(a T) T
        return a

fn make<T>() Op<T>
    return Impl<T>()

fn main()
    let o = make<String>()
    println(o.keep("a" + "b"))
"#,
        "ab",
    );
}

#[test]
fn every_explicit_type_argument_pins_its_own_parameter() {
    assert_heap_guard_output(
        r#"
class Pair<A, B>
    first A?
    second B?
    fn put(a A, b B) B
        self.first = a
        self.second = b
        return b

fn make<A, B>() Pair<A, B>
    return Pair<A, B>()

fn main()
    let p = make<int, String>()
    println(p.put(5, "a" + "b"))
"#,
        "ab",
    );
}

#[test]
fn an_explicit_type_argument_inside_a_generic_body_pins_the_callers_parameter() {
    assert_heap_guard_output(
        r#"
class Box<T>
    v T?
    fn put(a T) T
        self.v = a
        return a

fn make<T>() Box<T>
    return Box<T>()

fn relay<T>(a T) T
    let b = make<T>()
    return b.put(a)

fn main()
    println(relay("a" + "b"))
"#,
        "ab",
    );
}

#[test]
fn too_many_explicit_type_arguments_are_refused() {
    assert_compiler_error(
        r#"
use system.collections.list

fn make<T>() [T]
    return List<T>()

fn main()
    let xs = make<String, int>()
    println(f"{xs.length()}")
"#,
        "'make' expects 1 type argument, got 2",
    );
}

#[test]
fn a_wrong_count_of_explicit_type_arguments_is_reported_once() {
    let result = crate::utils::miri_check(
        r#"
use system.collections.list

fn make<T>() [T]
    return List<T>()

fn main()
    let xs = make<String, int>()
    println(f"{xs.length()}")
"#,
    );
    let output = result.output();
    assert!(
        output.contains("MER_TYP_036"),
        "expected MER_TYP_036:\n{output}"
    );
    assert_eq!(
        output.matches("error[").count(),
        1,
        "the refused call must not be reported again at its uses:\n{output}"
    );
}

/// Explicit type arguments on a function reached through a module alias bind
/// the same instantiation they do on a bare name.
#[test]
fn an_explicit_type_argument_through_a_module_alias_pins_the_call() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.boxes as B\n",
                    "\n",
                    "fn main()\n",
                    "    let b = B.make<String>()\n",
                    "    println(b.put(\"a\" + \"b\"))\n",
                ),
            ),
            (
                "boxes.mi",
                concat!(
                    "class Box<T>\n",
                    "    v T?\n",
                    "    fn put(a T) T\n",
                    "        self.v = a\n",
                    "        return a\n",
                    "\n",
                    "fn make<T>() Box<T>\n",
                    "    return Box<T>()\n",
                ),
            ),
        ],
        "ab",
    );
}

#[test]
fn an_explicit_type_argument_to_a_function_with_no_parameters_is_refused() {
    assert_compiler_error(
        r#"
fn one() int
    return 1

fn main()
    println(f"{one<int>()}")
"#,
        "'one' expects 0 type arguments, got 1",
    );
}

#[test]
fn an_explicit_type_argument_passes_a_sized_array_parameter() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn fill<T>(a Array<T, 4>) int
    return a.length()

fn main()
    let a = Array<int, 4>()
    println(f"{fill<int>(a)}")
"#,
        "4",
    );
}

#[test]
fn an_explicit_type_argument_returns_a_sized_array() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn make<T>() Array<T, 4>
    var a = Array<T, 4>()
    return a

fn main()
    println(f"{make<int>().length()}")
"#,
        "4",
    );
}

#[test]
fn a_sized_array_round_trips_through_a_generic_function() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn through<T>(a Array<T, 4>) Array<T, 4>
    return a

fn main()
    var a = Array<int, 4>()
    a[2] = 7
    let b = through<int>(a)
    println(f"{b[2]} {b.length()}")
"#,
        "7 4",
    );
}

/// A sized array holds its elements zeroed at construction, so its element
/// type has to be one a zero is a value of — a managed element is refused at
/// the declaration, not here. Two scalar widths are what distinguishes two
/// bodies: a body compiled for the wrong one reads the other's bits.
#[test]
fn two_element_types_produce_two_correct_bodies() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn through<T>(a Array<T, 4>) Array<T, 4>
    return a

fn main()
    var ints = Array<int, 4>()
    ints[0] = 9
    var reals = Array<f64, 4>()
    reals[0] = 1.5
    let a = through<int>(ints)
    let b = through<f64>(reals)
    println(f"{a[0]} {b[0]}")
"#,
        "9 1.5",
    );
}

/// The managed counterpart, through the unsized spelling a managed element is
/// allowed in.
#[test]
fn an_explicit_type_argument_round_trips_a_managed_element() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn through<T>(a [T]) [T]
    return a

fn main()
    var texts = List<String>()
    texts.push("h" + "i")
    let back = through<String>(texts)
    println(f"{back.length()} {back[0]}")
"#,
        "1 hi",
    );
}

#[test]
fn an_explicit_type_argument_on_a_managed_element_releases_it() {
    assert_heap_guard_output(
        r#"
fn ident<T>(a T) T
    return a

fn main()
    let s = ident<String>("h" + "i")
    println(s)
"#,
        "hi",
    );
}

const SHADOWED_ALIAS_MODULE: (&str, &str) = (
    "boxes.mi",
    "fn make() String\n    return \"MODULE-CALLED\"\n",
);

/// A local binding shadows a module alias of the same name: the call goes to
/// the binding, as any inner name shadows an outer one.
#[test]
fn a_local_binding_shadows_a_module_alias() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.boxes as B\n",
                    "\n",
                    "class Local\n",
                    "    fn make() String\n",
                    "        return \"LOCAL-CALLED\"\n",
                    "\n",
                    "fn main()\n",
                    "    let B = Local()\n",
                    "    println(B.make())\n",
                ),
            ),
            SHADOWED_ALIAS_MODULE,
        ],
        "LOCAL-CALLED",
    );
}

/// With type arguments written out, the shadowed call still never reaches
/// the module: explicit type arguments on a method call are not supported
/// yet, and the call is refused rather than sent to the alias.
#[test]
fn a_local_binding_shadows_a_module_alias_under_explicit_type_arguments() {
    assert_project_compiler_error(
        &[
            (
                "main.mi",
                concat!(
                    "use local.boxes as B\n",
                    "\n",
                    "class Local\n",
                    "    fn make<T>() String\n",
                    "        return \"LOCAL-CALLED\"\n",
                    "\n",
                    "fn main()\n",
                    "    let B = Local()\n",
                    "    println(B.make<String>())\n",
                ),
            ),
            SHADOWED_ALIAS_MODULE,
        ],
        "Expected generic function or type",
    );
}

const SHADOWING_CFG: &str = r#"
use system.math as M
use system.collections.list

class Cfg
    PI int
    fn init(v int)
        self.PI = v
"#;

/// A binding named like a module alias hides the alias: its field is read,
/// not the module's constant of the same name, whatever form binds it.
#[test]
fn a_local_named_like_a_module_alias_reads_its_own_field() {
    let code = format!(
        "{SHADOWING_CFG}
fn show(M Cfg)
    println(f\"{{M.PI}}\")

fn main()
    let M = Cfg(7)
    println(f\"{{M.PI}}\")
    show(Cfg(8))
    for M in List([Cfg(9)])
        println(f\"{{M.PI}}\")
    let o Cfg? = Cfg(10)
    match o
        Some(M): println(f\"{{M.PI}}\")
        None: println(\"none\")
    let f = fn(M Cfg) int: M.PI
    println(f\"{{f(Cfg(11))}}\")
"
    );
    assert_runs_with_output(&code, "7\n8\n9\n10\n11");
}
