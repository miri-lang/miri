// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! An operator written inside a generic body answers from the type the body was
//! instantiated at, not from the parameter the body was written against.
//!
//! Every element these tests compare is built at run time, so the answer cannot
//! come from the order the string-literal pool happens to lay literals out in.

use super::utils::*;

#[test]
fn test_list_of_strings_contains_answers_from_content() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<String>()
    l.push("PEAR".to_lower())
    l.push("APPLE".to_lower())
    let hit = "PEAR".to_lower()
    let miss = "FIG".to_lower()
    let found = l.contains(hit)
    let absent = l.contains(miss)
    println(f"found={found} absent={absent}")
"#,
        "found=true absent=false",
    );
}

#[test]
fn test_list_of_strings_index_of_answers_from_content() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<String>()
    l.push("PEAR".to_lower())
    l.push("APPLE".to_lower())
    let needle = "APPLE".to_lower()
    let at = l.index_of(needle)
    let missing = l.index_of("FIG".to_lower())
    println(f"at={at} missing={missing}")
"#,
        "at=Some(1) missing=None",
    );
}

#[test]
fn test_list_of_strings_min_and_max_answer_from_content() {
    // "pear" is pushed first, so an address comparison answers `min` with it.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<String>()
    l.push("PEAR".to_lower())
    l.push("APPLE".to_lower())
    l.push("FIG".to_lower())
    let smallest = l.min()
    let largest = l.max()
    println(f"min={smallest} max={largest}")
"#,
        "min=Some(apple) max=Some(pear)",
    );
}

#[test]
fn test_list_of_strings_is_empty_and_first_are_unaffected() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<String>()
    l.push("PEAR".to_lower())
    let empty = l.is_empty()
    let head = l.first()
    println(f"empty={empty} head={head}")
"#,
        "empty=false head=Some(pear)",
    );
}

#[test]
fn test_generic_function_orders_its_own_parameter_by_content() {
    assert_runs_with_output(
        r#"
fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

fn main()
    let first = "PEAR".to_lower()
    let second = "APPLE".to_lower()
    println(smaller(first, second))
"#,
        "apple",
    );
}

#[test]
fn test_generic_function_compares_its_own_parameter_for_equality() {
    assert_runs_with_output(
        r#"
fn same<T>(a T, b T) bool
    return a == b

fn main()
    let left = "PEAR".to_lower()
    let right = "PEAR".to_lower()
    let other = "FIG".to_lower()
    let alike = same(left, right)
    let different = same(left, other)
    println(f"alike={alike} different={different}")
"#,
        "alike=true different=false",
    );
}

#[test]
fn test_generic_function_still_orders_integers_by_value() {
    assert_runs_with_output(
        r#"
fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

fn main()
    println(f"{smaller(7, 3)}")
"#,
        "3",
    );
}

#[test]
fn test_generic_function_orders_a_user_type_by_its_own_rule() {
    assert_runs_with_output(
        r#"
use system.ops

class Weight implements Comparable
    grams int

    fn init(grams int)
        self.grams = grams

    public fn compare(other Weight) int
        return self.grams - other.grams

fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

fn main()
    let heavy = Weight(20)
    let light = Weight(10)
    let won = smaller(heavy, light)
    println(f"{won.grams}")
"#,
        "10",
    );
}

#[test]
fn test_instantiating_an_ordering_generic_with_an_unordered_type_is_refused() {
    assert_compiler_error(
        r#"
struct P
    v int

fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

fn main()
    let won = smaller(P(1), P(2))
    println(f"{won.v}")
"#,
        "MER_TYP_075",
    );
}

#[test]
fn test_min_over_a_list_of_an_unordered_type_is_refused() {
    assert_compiler_error(
        r#"
use system.collections.list

struct P
    v int

fn main()
    var l = List<P>()
    l.push(P(1))
    match l.min()
        Some(smallest): println(f"{smallest.v}")
        None: println("none")
"#,
        "MER_TYP_075",
    );
}

#[test]
fn test_a_trait_default_method_orders_through_its_own_parameter_name() {
    // The trait writes the comparison against `U`; the class binds `U` to its
    // own `T`, which the instantiation pins to `String`.
    assert_runs_with_output(
        r#"
trait Picker<U>
    fn one() U
    fn two() U

    public fn best() U
        let a = self.one()
        let b = self.two()
        if a < b
            return a
        return b

class Pair<T> implements Picker<T>
    a T
    b T

    fn init(a T, b T)
        self.a = a
        self.b = b

    public fn one() T: self.a
    public fn two() T: self.b

fn main()
    let words = Pair<String>("PEAR".to_lower(), "APPLE".to_lower())
    println(words.best())
"#,
        "apple",
    );
}

#[test]
fn test_a_trait_default_method_refuses_an_unordered_instantiation() {
    assert_compiler_error(
        r#"
struct P
    v int

trait Picker<U>
    fn one() U
    fn two() U

    public fn best() U
        let a = self.one()
        let b = self.two()
        if a < b
            return a
        return b

class Pair<T> implements Picker<T>
    a T
    b T

    fn init(a T, b T)
        self.a = a
        self.b = b

    public fn one() T: self.a
    public fn two() T: self.b

fn main()
    let ps = Pair<P>(P(1), P(2))
    let picked = ps.best()
    println(f"{picked.v}")
"#,
        "MER_TYP_075",
    );
}

#[test]
fn test_a_generic_class_method_orders_its_own_parameter() {
    assert_runs_with_output(
        r#"
class Box<T>
    v T

    fn init(v T)
        self.v = v

    public fn pick(other T) T
        if self.v < other
            return self.v
        return other

fn main()
    let box = Box<String>("PEAR".to_lower())
    println(box.pick("APPLE".to_lower()))
"#,
        "apple",
    );
}

#[test]
fn test_the_refusal_names_the_method_and_its_parameter() {
    assert_compiler_error(
        r#"
use system.collections.list

struct P
    v int

fn main()
    var l = List<P>()
    l.push(P(1))
    match l.min()
        Some(smallest): println(f"{smallest.v}")
        None: println("none")
"#,
        "'min' orders its 'T' parameter",
    );
}

#[test]
fn test_a_generic_body_that_orders_nothing_accepts_an_unordered_type() {
    // The requirement is per declaration: a body that never compares its
    // parameter constrains no instantiation.
    assert_runs_with_output(
        r#"
struct P
    v int

fn first_of<T>(a T, b T) T
    return a

fn main()
    let kept = first_of(P(7), P(9))
    println(f"{kept.v}")
"#,
        "7",
    );
}

#[test]
fn test_a_list_of_an_unordered_type_still_supports_its_other_methods() {
    // Only the methods that order the element type are constrained.
    assert_runs_with_output(
        r#"
use system.collections.list

struct P
    v int

fn main()
    var l = List<P>()
    l.push(P(1))
    l.push(P(2))
    let n = l.length()
    let holds = l.contains(P(2))
    println(f"n={n} holds={holds}")
"#,
        "n=2 holds=true",
    );
}
