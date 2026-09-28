// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method of a generic class is checked at a type argument when the program
//! uses it there: calls it, constructs or destroys an instance (`init`, the
//! drop hook), relies on it as an element's ordering or equality, or reaches it
//! through a trait the instance is converted to. A method never used at an
//! instantiation places no requirement on that argument.
//!
//! Each refused body would otherwise be compiled at a type nobody checked it
//! against — comparing a string's address with an integer — so the refusal is
//! what stands between the program and a crash or a wrong answer. Strings are
//! built at run time so an address comparison cannot pass for the right answer.

use super::utils::*;

const CANNOT_COMPARE: &str = "cannot compare String and int";

const INIT_ORDERS: &str = r#"
use system.io

class Box<T>
    v T
    fn init(v T)
        self.v = v
        println(f"{v < 10}")
"#;

fn with(declarations: &str, main: &str) -> String {
    format!("{declarations}\n{main}")
}

#[test]
fn an_init_ordering_its_parameter_is_refused_where_it_is_constructed() {
    assert_compiler_error(
        &with(
            INIT_ORDERS,
            "fn main()\n    let b = Box<String>(\"x\" + \"y\")\n    println(b.v)\n",
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn an_init_ordering_its_parameter_is_refused_by_a_build_too() {
    assert_build_error(
        &with(
            INIT_ORDERS,
            "fn main()\n    let b = Box<String>(\"x\" + \"y\")\n    println(b.v)\n",
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn an_init_comparing_its_parameter_for_equality_is_refused_where_it_is_constructed() {
    assert_compiler_error(
        r#"
use system.io

class Box<T>
    v T
    fn init(v T)
        self.v = v
        println(f"{v == 10}")

fn main()
    let b = Box<String>("x" + "y")
    println(b.v)
"#,
        CANNOT_COMPARE,
    );
}

#[test]
fn an_init_subtracting_from_its_parameter_is_refused_where_it_is_constructed() {
    assert_compiler_error(
        r#"
use system.io

class Box<T>
    v T
    fn init(v T)
        self.v = v
        let z = v - 1
        println(f"{z == v}")

fn main()
    let b = Box<String>("x" + "y")
    println(b.v)
"#,
        "'init' applies '-' to its 'T' parameter",
    );
}

#[test]
fn an_init_repeating_a_string_parameter_runs_at_the_string() {
    assert_heap_guard_output(
        r#"
use system.io

class Box<T>
    v T
    fn init(v T)
        self.v = v
        let z = v * 1
        println(f"{z == v}")

fn main()
    let b = Box<String>("x" + "y")
    println(b.v)
"#,
        "true\nxy",
    );
}

#[test]
fn a_drop_hook_ordering_its_parameter_is_refused_where_it_is_constructed() {
    assert_compiler_error(
        r#"
use system.io

class Box<T>
    v T
    fn drop(self)
        println(f"{self.v < 10}")

fn main()
    let b = Box<String>(v: "x" + "y")
    println(b.v)
"#,
        CANNOT_COMPARE,
    );
}

#[test]
fn a_drop_hook_a_child_inherits_is_refused_at_the_argument_its_extends_clause_pins() {
    assert_compiler_error(
        r#"
use system.io

class Box<T>
    v T
    fn drop(self)
        println(f"{self.v < 10}")

class Child extends Box<String>

fn main()
    let c = Child(v: "x" + "y")
    println(c.v)
"#,
        CANNOT_COMPARE,
    );
}

#[test]
fn a_construction_inside_a_generic_function_is_refused_at_the_functions_argument() {
    assert_compiler_error(
        &with(
            INIT_ORDERS,
            r#"
fn via<T>(v T) Box<T>
    return Box<T>(v)

fn main()
    let b = via("x" + "y")
    println(b.v)
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_drop_hook_reached_inside_a_generic_function_is_refused_at_the_functions_argument() {
    assert_compiler_error(
        r#"
use system.io

class Box<T>
    v T
    fn drop(self)
        println(f"{self.v < 10}")

fn via<T>(v T) Box<T>
    return Box<T>(v: v)

fn main()
    let b = via("x" + "y")
    println(b.v)
"#,
        CANNOT_COMPARE,
    );
}

#[test]
fn a_valid_instantiation_runs_its_init_and_drop_hook() {
    assert_heap_guard_output(
        r#"
use system.io

class Box<T>
    v T
    fn init(v T)
        self.v = v
        println(f"{v < 10}")
    fn drop(self)
        println(f"{self.v == 3}")

fn main()
    let b = Box<int>(3)
    println(f"{b.v}")
"#,
        "true\n3\ntrue",
    );
}

const LESS_THAN_TEN: &str = r#"
use system.io

trait Lt
    fn lt() bool

class Box<T> implements Lt
    v T
    fn lt() bool
        return self.v < 10
"#;

#[test]
fn a_method_reached_through_a_trait_object_is_refused_at_the_instances_argument() {
    assert_compiler_error(
        &with(
            LESS_THAN_TEN,
            r#"
fn go(x Lt) bool
    return x.lt()

fn main()
    let b = Box<String>(v: "x" + "y")
    println(f"{go(b)}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_method_reached_through_a_trait_object_is_refused_by_a_build_too() {
    assert_build_error(
        &with(
            LESS_THAN_TEN,
            r#"
fn go(x Lt) bool
    return x.lt()

fn main()
    let b = Box<String>(v: "x" + "y")
    println(f"{go(b)}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn an_instance_converted_to_a_trait_whose_method_is_never_called_is_accepted() {
    assert_heap_guard_output(
        &with(
            LESS_THAN_TEN,
            r#"
fn keep(x Lt) int
    return 1

fn main()
    let b = Box<String>(v: "x" + "y")
    println(f"{keep(b)}")
    println(b.v)
"#,
        ),
        "1\nxy",
    );
}

#[test]
fn a_method_reached_through_a_trait_object_runs_at_a_valid_argument() {
    assert_heap_guard_output(
        &with(
            LESS_THAN_TEN,
            r#"
fn go(x Lt) bool
    return x.lt()

fn main()
    let b = Box<int>(v: 3)
    println(f"{go(b)}")
"#,
        ),
        "true",
    );
}

#[test]
fn an_uncalled_method_places_no_requirement_on_its_argument() {
    assert_heap_guard_output(
        r#"
use system.io

class Box<T>
    v T
    fn lt() bool
        return self.v < 10

fn main()
    let b = Box<String>(v: "x" + "y")
    println(b.v)
"#,
        "xy",
    );
}

const ORDERED_BOX: &str = r#"
use system.io
use system.ops
use system.collections.list

class Box<T> implements Comparable
    v T
    public fn compare(_other Self) int
        if self.v < 10
            return -1
        return 1
"#;

#[test]
fn a_sort_relying_on_an_elements_compare_is_refused_at_the_elements_argument() {
    assert_compiler_error(
        &with(
            ORDERED_BOX,
            r#"
fn main()
    var l = List<Box<String>>()
    l.push(Box<String>(v: "x" + "y"))
    l.push(Box<String>(v: "x" + "z"))
    l.sort()
    println(l[0].v)
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn an_ordering_operator_on_instances_is_refused_at_the_instances_argument() {
    assert_compiler_error(
        &with(
            ORDERED_BOX,
            r#"
fn main()
    let a = Box<String>(v: "x" + "y")
    let b = Box<String>(v: "x" + "z")
    println(f"{a < b}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_list_of_instances_never_sorted_places_no_requirement_on_compare() {
    assert_heap_guard_output(
        &with(
            ORDERED_BOX,
            r#"
fn main()
    var l = List<Box<String>>()
    l.push(Box<String>(v: "x" + "y"))
    println(f"{l.length()}")
    println(l[0].v)
"#,
        ),
        "1\nxy",
    );
}

#[test]
fn a_list_of_a_class_without_ordering_is_accepted_when_nothing_sorts_it() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Dog
    name String

fn main()
    var l = List<Dog>()
    l.push(Dog(name: "re" + "x"))
    println(f"{l.length()}")
    println(l[0].name)
"#,
        "1\nrex",
    );
}

const EQUAL_BOX: &str = r#"
use system.io
use system.collections.list

class Box<T>
    v T
    public fn equals(_other Self) bool
        return self.v == 10
"#;

#[test]
fn a_search_relying_on_an_elements_equals_is_refused_at_the_elements_argument() {
    assert_compiler_error(
        &with(
            EQUAL_BOX,
            r#"
fn main()
    var l = List<Box<String>>()
    l.push(Box<String>(v: "x" + "y"))
    let found = l.contains(Box<String>(v: "x"))
    println(f"{found}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_generic_body_comparing_instances_is_refused_at_the_instances_argument() {
    assert_compiler_error(
        &with(
            EQUAL_BOX,
            r#"
fn same<T>(a T, b T) bool
    return a == b

fn main()
    let a = Box<String>(v: "x" + "y")
    println(f"{same(a, a)}")
"#,
        ),
        CANNOT_COMPARE,
    );
}

#[test]
fn a_generic_body_comparing_instances_runs_at_a_valid_argument() {
    assert_heap_guard_output(
        &with(
            EQUAL_BOX,
            r#"
fn same<T>(a T, b T) bool
    return a == b

fn main()
    let a = Box<int>(v: 10)
    println(f"{same(a, a)}")
"#,
        ),
        "true",
    );
}

/// A set matches its elements, and a map its keys, through the element
/// class's own `equals`, so every way a program adds to or looks up in one
/// runs that `equals` at the element's arguments.
const MATCHED_BOX: &str = r#"
use system.io
use system.collections.list
use system.collections.set
use system.collections.map

class Box<T>
    v T
    public fn equals(o Self) bool
        if self.v < 0
            return false
        return self.v == o.v
"#;

#[test]
fn adding_to_a_set_relying_on_an_elements_equals_is_refused_at_the_elements_argument() {
    assert_compiler_error(
        &with(
            MATCHED_BOX,
            r#"
fn main()
    var s = Set<Box<String>>()
    s.add(Box<String>(v: "x" + "y"))
    println(f"{s.length()}")
"#,
        ),
        "cannot compare String and int",
    );
}

#[test]
fn a_set_literal_of_instances_is_refused_at_the_elements_argument() {
    assert_compiler_error(
        &with(
            MATCHED_BOX,
            r#"
fn main()
    let s = {Box<String>(v: "x" + "y"), Box<String>(v: "x" + "z")}
    println(f"{s.length()}")
"#,
        ),
        "cannot compare String and int",
    );
}

#[test]
fn a_membership_test_on_a_set_is_refused_at_the_elements_argument() {
    assert_compiler_error(
        &with(
            MATCHED_BOX,
            r#"
fn main()
    let s = Set<Box<String>>()
    let b = Box<String>(v: "x" + "y")
    println(f"{b in s}")
"#,
        ),
        "cannot compare String and int",
    );
}

#[test]
fn setting_a_map_entry_is_refused_at_the_keys_argument() {
    assert_compiler_error(
        &with(
            MATCHED_BOX,
            r#"
fn main()
    var m = Map<Box<String>, int>()
    m.set(Box<String>(v: "x" + "y"), 1)
    println(f"{m.length()}")
"#,
        ),
        "cannot compare String and int",
    );
}

#[test]
fn a_map_literal_keyed_by_instances_is_refused_at_the_keys_argument() {
    assert_compiler_error(
        &with(
            MATCHED_BOX,
            r#"
fn main()
    let m = {Box<String>(v: "x" + "y"): 1}
    println(f"{m.length()}")
"#,
        ),
        "cannot compare String and int",
    );
}

#[test]
fn an_indexed_map_write_is_refused_at_the_keys_argument() {
    assert_compiler_error(
        &with(
            MATCHED_BOX,
            r#"
fn main()
    var m = Map<Box<String>, int>()
    m[Box<String>(v: "x" + "y")] = 3
    println(f"{m.length()}")
"#,
        ),
        "cannot compare String and int",
    );
}

#[test]
fn adding_to_a_set_inside_a_generic_function_is_refused_at_the_functions_argument() {
    assert_compiler_error(
        &with(
            MATCHED_BOX,
            r#"
fn put<T>(s Set<T>, v T)
    s.add(v)

fn main()
    var s = Set<Box<String>>()
    put(s, Box<String>(v: "x" + "y"))
    println(f"{s.length()}")
"#,
        ),
        "cannot compare String and int",
    );
}

/// At `int` the `equals` holds, and the set keeps one of two equal elements:
/// matching by the elements' bytes would keep both.
#[test]
fn a_set_of_instances_matches_them_by_their_equals_at_a_valid_argument() {
    assert_heap_guard_output(
        &with(
            MATCHED_BOX,
            r#"
fn main()
    var s = Set<Box<int>>()
    s.add(Box<int>(v: 3))
    s.add(Box<int>(v: 3))
    s.add(Box<int>(v: 4))
    println(f"{s.length()}")
"#,
        ),
        "2",
    );
}

#[test]
fn a_list_of_instances_never_matched_places_no_requirement_on_equals() {
    assert_heap_guard_output(
        &with(
            MATCHED_BOX,
            r#"
fn main()
    var l = List<Box<String>>()
    l.push(Box<String>(v: "x" + "y"))
    println(l[0].v)
"#,
        ),
        "xy",
    );
}

/// An optional element is matched through the `equals` of the value it
/// holds, so a set or map of optional instances runs that `equals` too.
#[test]
fn adding_to_a_set_of_optional_instances_is_refused_at_the_held_values_argument() {
    assert_compiler_error(
        &with(
            MATCHED_BOX,
            r#"
fn main()
    var s = Set<Box<String>?>()
    s.add(Box<String>(v: "x" + "y"))
    println(f"{s.length()}")
"#,
        ),
        "cannot compare String and int",
    );
}

#[test]
fn setting_a_map_entry_keyed_by_an_optional_instance_is_refused_at_the_held_values_argument() {
    assert_compiler_error(
        &with(
            MATCHED_BOX,
            r#"
fn main()
    var m = Map<Box<String>?, int>()
    m.set(Box<String>(v: "x" + "y"), 1)
    println(f"{m.length()}")
"#,
        ),
        "cannot compare String and int",
    );
}

#[test]
fn a_set_of_optional_instances_matches_them_by_their_equals_at_a_valid_argument() {
    assert_heap_guard_output(
        &with(
            MATCHED_BOX,
            r#"
fn main()
    var s = Set<Box<int>?>()
    s.add(Box<int>(v: 3))
    s.add(Box<int>(v: 3))
    s.add(Box<int>(v: 4))
    println(f"{s.length()}")
"#,
        ),
        "2",
    );
}

/// An instance stored where a class it extends is declared runs, through
/// its vtable, each method the program calls through that class.
const KID: &str = r#"
use system.io

abstract class Base
    public fn b() int
        return 0

class Kid<T> extends Base
    w T
    public fn b() int
        let d = self.w - 1
        return 9
"#;

#[test]
fn an_instance_stored_as_its_base_class_is_refused_where_the_base_calls_its_method() {
    assert_compiler_error(
        &with(
            KID,
            r#"
fn main()
    let k Base = Kid<String>(w: "x")
    println(f"{k.b()}")
"#,
        ),
        "Invalid types for arithmetic operation: String and int",
    );
}

#[test]
fn an_instance_passed_as_its_base_class_is_refused_where_the_base_calls_its_method() {
    assert_compiler_error(
        &with(
            KID,
            r#"
fn show(x Base) int
    return x.b()

fn main()
    let n = show(Kid<String>(w: "x"))
    println(f"{n}")
"#,
        ),
        "Invalid types for arithmetic operation: String and int",
    );
}

#[test]
fn an_instance_stored_as_its_base_class_runs_the_override_at_a_valid_argument() {
    assert_heap_guard_output(
        &with(
            KID,
            r#"
fn main()
    let k Base = Kid<int>(w: 3)
    println(f"{k.b()}")
"#,
        ),
        "9",
    );
}

/// An element's `equals` reaches the `equals` of what it holds only when a
/// set, map, or `==` asks for it; building the instance asks for neither.
#[test]
fn an_equals_nothing_asks_for_places_no_requirement_through_a_nested_instance() {
    assert_heap_guard_output(
        r#"
use system.io

class Inner<U>
    u U
    public fn equals(o Self) bool
        if self.u < 0
            return false
        return true

class Box<T>
    v T
    public fn equals(o Self) bool
        return self.v == o.v

fn main()
    let b = Box<Inner<String>>(v: Inner<String>(u: "x"))
    println("ok")
"#,
        "ok",
    );
}

/// A collection copies its `Cloneable` elements through their own `clone`,
/// explicitly or when a write to a shared collection copies it first, so
/// building a collection of instances uses their `clone` at its argument.
const CLONED_BOX: &str = r#"
use system.io
use system.memory
use system.collections.list
use system.collections.map

class Box<T> implements Cloneable
    v T
    w int
    public fn clone() Self
        let d = self.v - 1
        return Box<T>(v: self.v, w: self.w + 100)
"#;

#[test]
fn a_list_of_cloneable_instances_is_refused_at_the_elements_argument() {
    assert_compiler_error(
        &with(
            CLONED_BOX,
            r#"
fn main()
    var l = List<Box<String>>()
    l.push(Box<String>(v: "x", w: 1))
    let m = l.clone()
    println(f"{m[0].w}")
"#,
        ),
        "Invalid types for arithmetic operation: String and int",
    );
}

#[test]
fn a_map_of_cloneable_values_is_refused_at_the_values_argument() {
    assert_compiler_error(
        &with(
            CLONED_BOX,
            r#"
fn main()
    var m = Map<int, Box<String>>()
    m.set(1, Box<String>(v: "x", w: 1))
    println(f"{m.length()}")
"#,
        ),
        "Invalid types for arithmetic operation: String and int",
    );
}

/// The copy runs the `clone` compiled for the element's own argument.
#[test]
fn a_list_of_cloneable_instances_clones_them_at_a_valid_argument() {
    assert_heap_guard_output(
        &with(
            CLONED_BOX,
            r#"
fn main()
    var l = List<Box<i32>>()
    l.push(Box<i32>(v: 7, w: 1))
    let m = l.clone()
    println(f"{m[0].w} {m[0].v}")
"#,
        ),
        "101 7",
    );
}
