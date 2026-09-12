// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How `sort()` orders the elements it is given.
//!
//! A collection whose elements are values orders them by those values. A
//! collection whose elements are references orders them by what each reference
//! points at, which the element type answers through `compare`. Every element
//! compared here is built at run time, so the order allocations happen to fall
//! in cannot supply a right answer by accident.

use super::utils::{assert_compiler_error, assert_runs_with_output};

#[test]
fn test_sort_orders_runtime_built_strings_alphabetically() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var words = List<String>()
    words.push("PEAR".to_lower())
    words.push("APPLE".to_lower())
    words.push("FIG".to_lower())
    words.push("DATE".to_lower())
    words.sort()
    println(",".join(words))
"#,
        "apple,date,fig,pear",
    );
}

#[test]
fn test_sort_orders_strings_the_allocator_handed_out_in_reverse() {
    // Pushed in descending alphabetical order, so ascending allocation order
    // and ascending content order disagree on every pair.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var words = List<String>()
    words.push("ZEBRA".to_lower())
    words.push("YAK".to_lower())
    words.push("OWL".to_lower())
    words.push("ANT".to_lower())
    words.sort()
    println(",".join(words))
"#,
        "ant,owl,yak,zebra",
    );
}

#[test]
fn test_sort_orders_a_list_built_from_a_literal_of_strings() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var words = List(["pear".to_upper().to_lower(), "apple".to_upper().to_lower()])
    words.sort()
    println(",".join(words))
"#,
        "apple,pear",
    );
}

#[test]
fn test_sort_orders_an_array_of_strings_by_content() {
    assert_runs_with_output(
        r#"
fn main()
    var words = ["PEAR".to_lower(), "APPLE".to_lower(), "FIG".to_lower()]
    words.sort()
    println(f"{words[0]},{words[1]},{words[2]}")
"#,
        "apple,fig,pear",
    );
}

#[test]
fn test_sort_uses_the_element_types_own_compare() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Weight implements Comparable
    grams int

    fn init(grams int)
        self.grams = grams

    public fn compare(other Self) int
        return self.grams - other.grams

fn main()
    var l = List<Weight>()
    l.push(Weight(10))
    l.push(Weight(30))
    l.push(Weight(20))
    l.sort()
    println(f"{l[0].grams},{l[1].grams},{l[2].grams}")
"#,
        "10,20,30",
    );
}

#[test]
fn test_sort_uses_a_compare_that_orders_descending() {
    // The element type decides the order, so a `compare` written the other way
    // round is honoured rather than corrected.
    assert_runs_with_output(
        r#"
use system.collections.list

class Rank implements Comparable
    score int

    fn init(score int)
        self.score = score

    public fn compare(other Self) int
        return other.score - self.score

fn main()
    var l = List<Rank>()
    l.push(Rank(20))
    l.push(Rank(10))
    l.push(Rank(30))
    l.sort()
    println(f"{l[0].score},{l[1].score},{l[2].score}")
"#,
        "30,20,10",
    );
}

#[test]
fn test_sort_uses_the_compare_of_a_generic_element_class() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Box<T> implements Comparable
    rank int

    fn init(rank int)
        self.rank = rank

    public fn compare(other Self) int
        return self.rank - other.rank

fn main()
    var l = List<Box<int>>()
    l.push(Box<int>(20))
    l.push(Box<int>(10))
    l.push(Box<int>(30))
    l.sort()
    println(f"{l[0].rank},{l[1].rank},{l[2].rank}")
"#,
        "10,20,30",
    );
}

#[test]
fn test_a_generic_element_class_compares_at_its_own_instantiation() {
    // The comparator reaches the body compiled for `String`, not the shared one
    // written against the parameter — which would compare the two payloads by
    // address and answer with allocation order.
    assert_runs_with_output(
        r#"
use system.collections.list

class Tagged<T> implements Comparable
    value T

    fn init(value T)
        self.value = value

    public fn compare(other Self) int
        if self.value < other.value
            return -1
        if other.value < self.value
            return 1
        return 0

fn main()
    var l = List<Tagged<String>>()
    l.push(Tagged<String>("PEAR".to_lower()))
    l.push(Tagged<String>("APPLE".to_lower()))
    l.push(Tagged<String>("FIG".to_lower()))
    l.sort()
    println(f"{l[0].value},{l[1].value},{l[2].value}")
"#,
        "apple,fig,pear",
    );
}

#[test]
fn test_sort_uses_a_compare_an_abstract_base_defines() {
    // The element type is the abstract base, and the body answering for it is
    // the one that base compiled. What rules a type out of being compared is a
    // `compare` with no body, not an abstract declaration carrying one.
    assert_runs_with_output(
        r#"
use system.collections.list

public abstract class Shape implements Comparable
    area int

    fn init(area int)
        self.area = area

    public fn compare(other Self) int
        return self.area - other.area

class Square extends Shape
    fn init(area int)
        super.init(area)

fn main()
    var l = List<Shape>()
    l.push(Square(20))
    l.push(Square(10))
    l.push(Square(30))
    l.sort()
    println(f"{l[0].area},{l[1].area},{l[2].area}")
"#,
        "10,20,30",
    );
}

#[test]
fn test_sort_orders_a_list_a_transform_produced() {
    // The list `map` hands back carries the comparator of the element type it
    // was instantiated at.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var words = List<String>()
    words.push("PEAR")
    words.push("APPLE")
    words.push("FIG")
    var lowered = words.map(fn(w String) String: w.to_lower())
    lowered.sort()
    println(",".join(lowered))
"#,
        "apple,fig,pear",
    );
}

#[test]
fn test_sort_of_a_cloned_list_of_strings_orders_by_content() {
    // The clone carries the element comparator the source was given.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var words = List<String>()
    words.push("PEAR".to_lower())
    words.push("APPLE".to_lower())
    var copy = words.clone()
    copy.sort()
    println(",".join(copy))
"#,
        "apple,pear",
    );
}

#[test]
fn test_sort_of_one_string_and_of_none_are_left_alone() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var empty = List<String>()
    empty.sort()
    var one = List<String>()
    one.push("PEAR".to_lower())
    one.sort()
    println(f"{empty.length()},{one.length()},{one[0]}")
"#,
        "0,1,pear",
    );
}

#[test]
fn test_sort_keeps_equal_strings_and_orders_around_them() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var words = List<String>()
    words.push("PEAR".to_lower())
    words.push("APPLE".to_lower())
    words.push("PEAR".to_lower())
    words.push("APPLE".to_lower())
    words.sort()
    println(",".join(words))
"#,
        "apple,apple,pear,pear",
    );
}

#[test]
fn test_sorting_a_struct_without_ordering_is_refused() {
    assert_compiler_error(
        r#"
use system.collections.list

struct P
    v int

fn main()
    var l = List<P>()
    l.push(P(3))
    l.push(P(1))
    l.sort()
"#,
        "MER_TYP_075",
    );
}

#[test]
fn test_sorting_a_class_without_ordering_is_refused() {
    assert_compiler_error(
        r#"
use system.collections.list

class Box
    v int

    fn init(v int)
        self.v = v

fn main()
    var l = List<Box>()
    l.push(Box(3))
    l.sort()
"#,
        "MER_TYP_075",
    );
}

#[test]
fn test_sorting_an_array_without_ordering_is_refused() {
    assert_compiler_error(
        r#"
struct P
    v int

fn main()
    var a = [P(3), P(1)]
    a.sort()
"#,
        "MER_TYP_075",
    );
}

#[test]
fn test_sorting_a_list_of_lists_is_refused() {
    assert_compiler_error(
        r#"
use system.collections.list

fn main()
    var rows = List<[int]>()
    rows.push(List([2, 1]))
    rows.sort()
"#,
        "MER_TYP_075",
    );
}

#[test]
fn test_each_element_type_takes_the_path_its_values_allow() {
    // One program, both paths. The integers are ordered by the bytes the list
    // stores, which for them is the value; the strings and the class instances
    // are references, so each is ordered by a comparator calling the element
    // type's own `compare`. A type that answers neither is not sorted at all —
    // it is refused, which the rejection tests above pin.
    assert_runs_with_output(
        r#"
use system.collections.list

class Weight implements Comparable
    grams int

    fn init(grams int)
        self.grams = grams

    public fn compare(other Self) int
        return self.grams - other.grams

fn main()
    var numbers = List<int>()
    numbers.push(30)
    numbers.push(10)
    numbers.push(20)
    numbers.sort()

    var words = List<String>()
    words.push("PEAR".to_lower())
    words.push("APPLE".to_lower())

    words.sort()

    var weights = List<Weight>()
    weights.push(Weight(20))
    weights.push(Weight(10))
    weights.sort()

    println(f"{numbers[0]},{numbers[1]},{numbers[2]} {words[0]},{words[1]} {weights[0].grams},{weights[1].grams}")
"#,
        "10,20,30 apple,pear 10,20",
    );
}

#[test]
fn test_ints_sort_by_value() {
    // Integer elements are the bytes the list stores, so they take the path
    // that reads those bytes as a number rather than a comparator call.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<int>()
    l.push(30)
    l.push(-10)
    l.push(20)
    l.sort()
    println(f"{l[0]},{l[1]},{l[2]}")
"#,
        "-10,20,30",
    );
}

#[test]
fn test_floats_sort_by_value() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<float>()
    l.push(3.5)
    l.push(-1.5)
    l.push(2.25)
    l.sort()
    println(f"{l[0]},{l[1]},{l[2]}")
"#,
        "-1.5,2.25,3.5",
    );
}

#[test]
fn test_booleans_sort_by_value() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<bool>()
    l.push(true)
    l.push(false)
    l.push(true)
    l.sort()
    println(f"{l[0]},{l[1]},{l[2]}")
"#,
        "false,true,true",
    );
}
