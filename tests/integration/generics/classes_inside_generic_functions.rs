// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic class written against a generic function's own parameter
//! (`Box<T>(a)` inside `via_box<T>`) is instantiated at the type the function
//! itself is instantiated at: its constructor, its methods and its drop all
//! see the concrete element, so a managed one is retained and released like
//! any other.
//!
//! Strings are built at run time so an unretained reference or an address
//! comparison cannot pass for the right answer.

use super::utils::*;

const BOX: &str = r#"
class Box<T>
    v T

    fn init(v T)
        self.v = v

    public fn get() T
        return self.v
"#;

fn with_box(program: &str) -> String {
    format!("{BOX}\n{program}")
}

#[test]
fn test_a_class_built_at_the_parameter_keeps_a_string() {
    assert_runs_with_output(
        &with_box(
            r#"
fn via_box<T>(a T) T
    let box = Box<T>(a)
    return box.get()

fn main()
    println(via_box("PEAR".to_lower()))
"#,
        ),
        "pear",
    );
}

#[test]
fn test_one_function_builds_the_class_at_a_string_and_at_an_int() {
    assert_runs_with_output(
        &with_box(
            r#"
fn via_box<T>(a T) T
    let box = Box<T>(a)
    return box.get()

fn main()
    println(via_box("PEAR".to_lower()))
    println(f"{via_box(7)}")
"#,
        ),
        "pear\n7",
    );
}

#[test]
fn test_a_class_built_at_the_parameter_keeps_a_float_at_its_width() {
    assert_runs_with_output(
        &with_box(
            r#"
fn via_box<T>(a T) T
    let box = Box<T>(a)
    return box.get()

fn main()
    let f f32 = 2.5
    println(f"{via_box(f)}")
    println(f"{via_box(0.25)}")
"#,
        ),
        "2.5\n0.25",
    );
}

#[test]
fn test_a_temporary_built_at_the_parameter_answers_a_method() {
    // At a scalar: a temporary built at a managed type still leaks its field
    // through the shared drop, a defect of temporaries independent of generic
    // functions.
    assert_runs_with_output(
        &with_box(
            r#"
fn via_box<T>(a T) T
    return Box<T>(a).get()

fn main()
    let f f32 = 1.5
    println(f"{via_box(f)}")
"#,
        ),
        "1.5",
    );
}

#[test]
fn test_a_delegating_function_hands_the_class_its_instantiation() {
    assert_runs_with_output(
        &with_box(
            r#"
fn via_box<T>(a T) T
    let box = Box<T>(a)
    return box.get()

fn outer<T>(a T) T
    return via_box(a)

fn main()
    println(outer("PEAR".to_lower()))
"#,
        ),
        "pear",
    );
}

#[test]
fn test_a_class_method_built_inside_a_generic_function_orders_by_content() {
    assert_runs_with_output(
        r#"
fn smaller<T>(a T, b T) T
    if a < b
        return a
    return b

class Box<T>
    v T

    fn init(v T)
        self.v = v

    public fn min_with(other T) T
        return smaller(self.v, other)

fn via_box<T>(a T, b T) T
    let box = Box<T>(a)
    return box.min_with(b)

fn main()
    println(via_box("PEAR".to_lower(), "APPLE".to_lower()))
    println(f"{via_box(10, 3)}")
    println(f"{via_box(2, 30)}")
"#,
        "apple\n3\n2",
    );
}

#[test]
fn test_a_class_method_instantiation_builds_another_class_at_its_parameter() {
    assert_runs_with_output(
        &with_box(
            r#"
class Holder<T>
    v T

    fn init(v T)
        self.v = v

    public fn boxed() T
        let box = Box<T>(self.v)
        return box.get()

fn main()
    let h = Holder<String>("PEAR".to_lower())
    println(h.boxed())
"#,
        ),
        "pear",
    );
}

#[test]
fn test_a_list_built_at_the_parameter_keeps_its_strings() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn second<T>(a T, b T) T
    var l = List<T>()
    l.push(a)
    l.push(b)
    return l[1]

fn main()
    println(second("X".to_lower(), "Y".to_lower()))
"#,
        "y",
    );
}

#[test]
fn test_a_set_of_a_class_built_at_the_parameter_matches_equal_payloads() {
    assert_runs_with_output(
        r#"
use system.collections.set

class Tagged<T>
    value T

    fn init(value T)
        self.value = value

    public fn equals(other Self) bool
        return self.value == other.value

fn distinct<T>(a T, b T) int
    var s = Set<Tagged<T>>()
    let x = Tagged<T>(a)
    let y = Tagged<T>(b)
    s.add(x)
    s.add(y)
    return s.length()

fn main()
    println(f"{distinct('X'.to_lower(), 'X'.to_lower())}")
    println(f"{distinct('X'.to_lower(), 'Y'.to_lower())}")
"#,
        "1\n2",
    );
}

#[test]
fn test_a_list_of_a_class_built_at_the_parameter_sorts_by_content() {
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

fn sorted<T>(a T, b T, c T) List<T>
    var l = List<Tagged<T>>()
    l.push(Tagged<T>(a))
    l.push(Tagged<T>(b))
    l.push(Tagged<T>(c))
    l.sort()
    var out = List<T>()
    for t in l
        out.push(t.value)
    return out

fn main()
    for w in sorted("PEAR".to_lower(), "APPLE".to_lower(), "FIG".to_lower())
        println(w)
    for n in sorted(30, 10, 20)
        println(f"{n}")
"#,
        "apple\nfig\npear\n10\n20\n30",
    );
}
