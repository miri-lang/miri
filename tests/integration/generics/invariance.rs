// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A value is read and written through the type of the location that holds
//! it, so the type arguments of that location must name exactly what the
//! value was built at: a collection's element, key and value types, and a
//! function value's parameters and result. Handing on a `List<Dog>` as a
//! `List<Animal>` would let an `Animal` be pushed where `Dog` readers look,
//! and a `List<i8>` as a `List<i64>` would write 8-byte elements into 1-byte
//! slots. A literal is built at the location's type, so it may still be
//! written where a wider element is declared.

use super::utils::*;

const ANIMALS: &str = r#"
use system.collections.list
use system.collections.array
use system.collections.set
use system.collections.map

class Animal
    n int
    fn init(n int)
        self.n = n

class Dog extends Animal
    tag String
    fn init(t String)
        super.init(1)
        self.tag = t
"#;

#[test]
fn a_list_is_invariant_in_its_element_type() {
    let code = format!(
        "{ANIMALS}
fn fill(xs List<Animal>)
    xs.push(Animal(5))

fn main()
    let xs = List<Dog>()
    fill(xs)
"
    );
    assert_compiler_error(
        &code,
        "Type mismatch for argument 'xs': expected List<Animal>, got List<Dog>",
    );
}

#[test]
fn an_array_is_invariant_in_its_element_type() {
    let code = format!(
        "{ANIMALS}
fn fill(xs Array<Animal, 1>)
    xs.set(0, Animal(5))

fn main()
    let xs = Array<Dog, 1>(Dog(\"hello\"))
    fill(xs)
"
    );
    assert_compiler_error(
        &code,
        "Type mismatch for argument 'xs': expected Array<Animal, 1>, got Array<Dog, 1>",
    );
}

#[test]
fn a_set_is_invariant_in_its_element_type() {
    let code = format!(
        "{ANIMALS}
fn count(xs Set<i64>) int
    return xs.length()

fn main()
    let xs = Set<i8>()
    println(f\"{{count(xs)}}\")
"
    );
    assert_compiler_error(
        &code,
        "Type mismatch for argument 'xs': expected Set<i64>, got Set<i8>",
    );
}

#[test]
fn a_map_is_invariant_in_its_value_type() {
    let code = format!(
        "{ANIMALS}
fn fill(m Map<String, Animal>)
    m.set(\"k\", Animal(5))

fn main()
    let m = Map<String, Dog>()
    fill(m)
"
    );
    assert_compiler_error(
        &code,
        "Type mismatch for argument 'm': expected Map<String, Animal>, got Map<String, Dog>",
    );
}

#[test]
fn a_map_is_invariant_in_its_key_type() {
    let code = format!(
        "{ANIMALS}
fn count(m Map<i64, int>) int
    return m.length()

fn main()
    let m = Map<i8, int>()
    println(f\"{{count(m)}}\")
"
    );
    assert_compiler_error(
        &code,
        "Type mismatch for argument 'm': expected Map<i64, int>, got Map<i8, int>",
    );
}

/// A narrower integer element would be written at the wider width.
#[test]
fn a_list_of_a_narrower_integer_is_not_a_list_of_a_wider_one() {
    assert_compiler_error(
        r#"
use system.collections.list

fn fill(xs List<i64>)
    xs.push(1099511627776)

fn main()
    let xs = List<i8>()
    fill(xs)
"#,
        "Type mismatch for argument 'xs': expected List<i64>, got List<i8>",
    );
}

/// `int` is a type of its own, not a stand-in for every integer: a list of
/// another width read as a `List<int>` would be read at the wrong stride.
#[test]
fn a_list_of_another_integer_width_is_not_a_list_of_int() {
    for element in ["i32", "i64", "i128", "u8"] {
        assert_compiler_error(
            &format!(
                r#"
use system.collections.list

fn show(xs List<int>)
    println(f"{{xs[0]}}")

fn main()
    let xs List<{element}> = List([1, 2, 3])
    show(xs)
"#
            ),
            &format!("expected List<int>, got List<{element}>"),
        );
    }
}

/// A `float` argument is no more a stand-in for every float width.
#[test]
fn a_list_of_another_float_width_is_not_a_list_of_float() {
    assert_compiler_error(
        r#"
use system.collections.list

fn show(xs List<float>)
    println(f"{xs[0]}")

fn main()
    let xs List<f32> = List([1.5, 2.5])
    show(xs)
"#,
        "expected List<float>, got List<f32>",
    );
}

const WIDE: &str = r#"
enum E<A, B>
    L(A)
    R(B, A)
"#;

/// An enum argument fixes its payload slot width the same way.
#[test]
fn an_enum_at_another_integer_argument_is_refused_either_way() {
    for (param, value) in [("int", "i128"), ("i8", "int")] {
        assert_compiler_error(
            &format!(
                r#"{WIDE}
fn show(e E<String, {param}>)
    match e
        E.R(n, s): println(f"r {{s}} {{n}}")
        E.L(s): println(f"l {{s}}")

fn make(n {value}) E<String, {value}>
    return E.R(n, "t" + "u")

fn main()
    show(make(7))
"#
            ),
            &format!("expected E<String, {param}>, got E<String, {value}>"),
        );
    }
}

/// A literal payload is built where it is written, so the variant takes the
/// width the location declares; a payload read from elsewhere does not.
#[test]
fn a_literal_payload_takes_the_declared_argument_width() {
    assert_heap_guard_output(
        &format!(
            r#"{WIDE}
fn show(e E<String, i128>)
    match e
        E.R(n, s): println(f"r {{s}} {{n}}")
        E.L(s): println(f"l {{s}}")

fn make() E<String, i128>
    return E.R(5, "t" + "u")

fn main()
    let e E<String, i128> = E.R(-5000000000, "m" + "n")
    show(e)
    show(make())
    show(E.R(7, "c" + "d"))
"#
        ),
        "r mn -5000000000\nr tu 5\nr cd 7",
    );
}

/// A literal-payload variant nested in another value built in place takes
/// the declared width too: the payload of an optional, and the elements of a
/// literal a typed collection constructor copies.
#[test]
fn a_nested_literal_payload_takes_the_declared_argument_width() {
    assert_heap_guard_output(
        &format!(
            r#"{WIDE}
use system.collections.list

fn show(e E<String, i128>)
    match e
        E.R(n, s): println(f"r {{s}} {{n}}")
        E.L(s): println(f"l {{s}}")

fn opt(o Option<E<String, i128>>)
    match o
        Some(e): show(e)
        None: println("none")

fn main()
    opt(Some(E.R(9, "s" + "")))
    let wrapped Option<E<String, i128>> = Some(E.R(8, "w" + ""))
    opt(wrapped)
    let xs = List<E<String, i128>>([E.R(3, "a" + ""), E.R(4, "b" + "")])
    for x in xs
        show(x)
"#
        ),
        "r s 9\nr w 8\nr a 3\nr b 4",
    );
}

#[test]
fn a_nested_variable_payload_does_not_take_the_declared_argument_width() {
    assert_compiler_error(
        &format!(
            r#"{WIDE}
fn opt(o Option<E<String, i128>>)
    println("x")

fn pass(n int)
    opt(Some(E.R(n, "s" + "")))
"#
        ),
        "Type Mismatch",
    );
}

#[test]
fn a_variable_payload_does_not_take_the_declared_argument_width() {
    assert_compiler_error(
        &format!(
            r#"{WIDE}
fn make(n int) E<String, i128>
    return E.R(n, "t" + "u")

fn main()
    let e = make(5)
"#
        ),
        "Type Mismatch",
    );
}

/// An optional element is laid out wider than the value it wraps.
#[test]
fn a_list_of_values_is_not_a_list_of_optionals() {
    assert_compiler_error(
        r#"
use system.collections.list

fn fill(xs List<int?>)
    xs.push(None)

fn main()
    let xs = List<int>()
    fill(xs)
"#,
        "Type mismatch for argument 'xs': expected List<int?>, got List<int>",
    );
}

#[test]
fn a_function_taking_a_subtype_is_not_one_taking_its_supertype() {
    let code = format!(
        "{ANIMALS}
fn handle_dog(d Dog) String
    return d.tag

fn feed(f fn(a Animal) String) String
    return f(Animal(9))

fn main()
    println(feed(handle_dog))
"
    );
    assert_compiler_error(&code, "Type mismatch for argument 'f'");
}

/// Parameters are invariant rather than contravariant: the value is passed at
/// the stored signature's layout, which an optional parameter does not share.
#[test]
fn a_function_taking_a_supertype_is_not_one_taking_its_subtype() {
    let code = format!(
        "{ANIMALS}
fn handle_animal(a Animal) int
    return a.n

fn feed(f fn(d Dog) int) int
    return f(Dog(\"x\"))

fn main()
    println(f\"{{feed(handle_animal)}}\")
"
    );
    assert_compiler_error(&code, "Type mismatch for argument 'f'");
}

#[test]
fn a_function_returning_a_value_is_not_one_returning_an_optional() {
    assert_compiler_error(
        r#"
fn three() int
    return 3

fn read(f fn() int?) int
    let v = f()
    match v
        Some(n): println(f"{n}")
        None: println("none")
    return 0

fn main()
    println(f"{read(three)}")
"#,
        "Type mismatch for argument 'f'",
    );
}

/// A literal is built at the type of the location it is written into, so its
/// element type is not held to the literal's own inferred one.
#[test]
fn a_literal_is_built_at_the_declared_element_type() {
    assert_heap_guard_output(
        r#"
use system.collections.array
use system.collections.map

fn main()
    let ys Array<i8, 2> = [5, 6]
    let m Map<String, int> = {}
    let n Map<String, i64> = {"a": 1}
    println(f"{ys[1]} {m.length()} {n.length()}")
"#,
        "6 0 1",
    );
}

#[test]
fn a_literal_holding_a_value_the_element_type_refuses_is_refused() {
    assert_compiler_error(
        r#"
use system.collections.map

fn main()
    var m {String: int} = {"a": None}
"#,
        "Type mismatch for variable 'm'",
    );
}

/// Only an element that builds its own value is built at the declared element
/// type. A variable already holds its own layout, so a narrower integer read
/// from one is refused rather than stored into a wider slot unconverted.
#[test]
fn a_narrower_integer_variable_in_a_literal_is_refused() {
    assert_compiler_error(
        r#"
use system.collections.array

fn main()
    let b i8 = 5
    let c i8 = -3
    let xs Array<i64, 2> = [b, c]
    println(f"{xs[0]} {xs[1]}")
"#,
        "expected Array<i64, 2>, got Array<i8, 2>",
    );
}

#[test]
fn a_narrower_integer_variable_in_a_collection_constructor_is_refused() {
    assert_compiler_error(
        r#"
use system.collections.list

fn main()
    let b i8 = -3
    let xs = List<i64>([b, b])
    println(f"{xs[0]}")
"#,
        "MER_TYP_052",
    );
}

#[test]
fn a_tuple_variable_in_a_literal_of_wider_tuples_is_refused() {
    assert_compiler_error(
        r#"
use system.collections.array

fn main()
    let t = (7, "a")
    let xs Array<(int?, String), 1> = [t]
    println(xs[0].1)
"#,
        "Type Mismatch",
    );
}

/// A function type's nullable result keeps its `?` in the message, so the two
/// sides of the mismatch do not read the same.
#[test]
fn a_function_type_mismatch_spells_a_nullable_result() {
    assert_compiler_error(
        r#"
fn inc(x int) int
    return x + 1

fn make() fn(x int) int?
    return inc

fn main()
    let f = make()
    println(f"{f(1)}")
"#,
        "expected Function(int) -> int?, got Function(int) -> int",
    );
}

/// A generic parameter reached through a function-typed argument is bound
/// from every argument before any is checked, so the order the arguments are
/// written in does not decide whether the call is accepted.
#[test]
fn a_lambda_argument_before_the_value_that_binds_its_parameter_is_accepted() {
    assert_runs_with_output(
        r#"
fn apply<T>(f fn(x T) T, v T) T
    return f(v)

fn inc(x int) int
    return x + 1

fn main()
    println(f"{apply(fn(x int) int: x + 1, 3)} {apply(inc, 4)}")
"#,
        "4 5",
    );
}

#[test]
fn a_lambda_result_binds_the_parameter_only_it_mentions() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn map_all<T, U>(xs List<T>, f fn(x T) U) List<U>
    var out = List<U>()
    for x in xs
        out.push(f(x))
    return out

fn main()
    let ys = map_all(List([1, 2]), fn(x int) String: f"{x}!")
    println(ys[1])
"#,
        "2!",
    );
}

#[test]
fn a_lambda_disagreeing_with_the_other_arguments_is_refused() {
    assert_compiler_error(
        r#"
fn apply<T>(f fn(x T) T, v T) T
    return f(v)

fn main()
    println(apply(fn(x String) String: x, 3))
"#,
        "Type Mismatch",
    );
}
