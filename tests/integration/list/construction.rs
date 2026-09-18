// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn list_construction_int() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List([1, 2, 3])
println(f\"{l.length()}\")
",
        "3",
    );
}

#[test]
fn list_construction_string() {
    assert_runs_with_output(
        "
use system.collections.list

let l = List([\"hello\", \"world\"])
println(f\"{l.length()}\")
",
        "2",
    );
}

#[test]
fn list_from_array_variable_keeps_run_time_strings() {
    assert_runs_with_output(
        "
use system.collections.list

fn main()
    var a = [\"PEAR\".to_lower(), \"APPLE\".to_lower()]
    var fromArray = List(a)
    println(f\"{fromArray[0]}|{fromArray[1]}|{fromArray.length()}\")
",
        "pear|apple|2",
    );
}

#[test]
fn list_from_array_variable_leaves_the_array_readable() {
    assert_runs_with_output(
        "
use system.collections.list

fn main()
    var a = [\"PEAR\".to_lower(), \"APPLE\".to_lower()]
    var fromArray = List(a)
    println(f\"{a[0]}|{a[1]}|{fromArray[1]}|{fromArray[0]}\")
",
        "pear|apple|apple|pear",
    );
}

#[test]
fn list_from_array_variable_of_ints_leaves_the_array_readable() {
    assert_runs_with_output(
        "
use system.collections.list

fn main()
    let a = [3, 5, 7]
    var l = List(a)
    l.push(9)
    println(f\"{a[0]}{a[1]}{a[2]}|{l[0]}{l[1]}{l[2]}{l[3]}|{l.length()}\")
",
        "357|3579|4",
    );
}

#[test]
fn list_from_array_variable_keeps_class_elements() {
    assert_runs_with_output(
        "
use system.collections.list

class Fruit
    var name String

fn main()
    var a = [Fruit(name: \"PEAR\".to_lower()), Fruit(name: \"APPLE\".to_lower())]
    var l = List(a)
    println(f\"{l[0].name}|{l[1].name}|{a[0].name}\")
",
        "pear|apple|pear",
    );
}

#[test]
fn list_from_array_variable_keeps_nested_collection_elements() {
    assert_runs_with_output(
        "
use system.collections.list

fn main()
    var a = [List([\"PEAR\".to_lower()]), List([\"APPLE\".to_lower(), \"FIG\".to_lower()])]
    var l = List(a)
    println(f\"{l[0][0]}|{l[1][1]}|{l[1].length()}|{a[1][0]}\")
",
        "pear|fig|2|apple",
    );
}

#[test]
fn list_from_array_parameter_outlives_a_temporary_argument() {
    assert_runs_with_output(
        "
use system.collections.list

fn to_list(a [String; 2]) [String]
    return List(a)

fn main()
    let a = [\"PEAR\".to_lower(), \"APPLE\".to_lower()]
    let l = to_list(a)
    let t = to_list([\"FIG\".to_lower(), \"KIWI\".to_lower()])
    println(f\"{l[0]}|{l[1]}|{a[0]}|{t[0]}|{t[1]}\")
",
        "pear|apple|pear|fig|kiwi",
    );
}

#[test]
fn list_from_array_field_keeps_the_owner_readable() {
    assert_runs_with_output(
        "
use system.collections.list

class Basket
    var items [String; 2]

fn main()
    let b = Basket(items: [\"PEAR\".to_lower(), \"APPLE\".to_lower()])
    let l = List(b.items)
    println(f\"{l[0]}|{l[1]}|{b.items[1]}\")
",
        "pear|apple|apple",
    );
}

#[test]
fn list_from_the_same_array_in_a_loop_releases_each_list() {
    assert_runs_with_output(
        "
use system.collections.list

fn main()
    let a = [\"PEAR\".to_lower(), \"APPLE\".to_lower()]
    var total = 0
    var i = 0
    while i < 20
        var l = List(a)
        l.push(\"FIG\".to_lower())
        total = total + l.length()
        i = i + 1
    println(f\"{total}|{a[0]}|{a[1]}\")
",
        "60|pear|apple",
    );
}

#[test]
fn list_from_array_returned_by_a_call_keeps_its_strings() {
    assert_runs_with_output(
        "
use system.collections.list

fn fruits() [String; 2]
    return [\"PEAR\".to_lower(), \"APPLE\".to_lower()]

fn main()
    var l = List(fruits())
    println(f\"{l[0]}|{l[1]}\")
",
        "pear|apple",
    );
}

#[test]
fn list_from_list_of_ints_copies_every_element() {
    assert_runs_with_output(
        "
use system.collections.list

fn main()
    let ints = List([1, 2, 3])
    var copy = List(ints)
    copy.push(4)
    copy[0] = 9
    println(f\"{ints.length()}|{ints[0]}{ints[1]}{ints[2]}|{copy.length()}|{copy[0]}{copy[1]}{copy[2]}{copy[3]}\")
",
        "3|123|4|9234",
    );
}

#[test]
fn list_from_list_of_run_time_strings_is_independent_of_its_source() {
    assert_heap_guard_output(
        "
use system.collections.list

fn main()
    var src = List([\"PEAR\".to_lower(), \"APPLE\".to_lower()])
    var copy = List(src)
    copy.push(\"FIG\".to_lower())
    src[0] = \"KIWI\".to_lower()
    println(f\"{src.length()}|{src[0]}|{src[1]}|{copy.length()}|{copy[0]}|{copy[1]}|{copy[2]}\")
",
        "2|kiwi|apple|3|pear|apple|fig",
    );
}

#[test]
fn list_from_list_outlives_a_source_that_goes_out_of_scope() {
    assert_heap_guard_output(
        "
use system.collections.list

fn copied() [String]
    let src = List([\"PEAR\".to_lower(), \"APPLE\".to_lower()])
    return List(src)

fn main()
    let l = copied()
    println(f\"{l[0]}|{l[1]}|{l.length()}\")
",
        "pear|apple|2",
    );
}

#[test]
fn list_from_list_shares_class_elements_like_clone() {
    assert_heap_guard_output(
        "
use system.collections.list

class Fruit
    var name String

fn main()
    var src = List([Fruit(name: \"PEAR\".to_lower()), Fruit(name: \"APPLE\".to_lower())])
    var copy = List(src)
    copy.push(Fruit(name: \"FIG\".to_lower()))
    copy[0].name = \"KIWI\".to_lower()
    println(f\"{src.length()}|{src[0].name}|{copy.length()}|{copy[0].name}|{copy[2].name}\")
",
        "2|kiwi|3|kiwi|fig",
    );
}

#[test]
fn list_from_list_copies_cloneable_elements_like_clone() {
    assert_heap_guard_output(
        "
use system.collections.list

class Counter implements Cloneable
    var value int

    public fn clone() Self
        return Counter(value: self.value)

fn main()
    var src = List([Counter(value: 1), Counter(value: 2)])
    var copy = List(src)
    copy[0].value = 7
    println(f\"{src[0].value}|{copy[0].value}|{copy[1].value}\")
",
        "1|7|2",
    );
}

#[test]
fn list_from_list_keeps_nested_collection_elements() {
    assert_heap_guard_output(
        "
use system.collections.list

fn main()
    var src = List([List([\"PEAR\".to_lower()]), List([\"APPLE\".to_lower(), \"FIG\".to_lower()])])
    var copy = List(src)
    copy.remove_at(0)
    println(f\"{src.length()}|{src[0][0]}|{copy.length()}|{copy[0][1]}\")
",
        "2|pear|1|fig",
    );
}

#[test]
fn list_from_list_returned_by_a_call_releases_the_temporary() {
    assert_runs_with_output(
        "
use system.collections.list

fn fruits() [String]
    return List([\"PEAR\".to_lower(), \"APPLE\".to_lower()])

fn main()
    var l = List(fruits())
    l.push(\"FIG\".to_lower())
    println(f\"{l[0]}|{l[2]}|{l.length()}\")
",
        "pear|fig|3",
    );
}

#[test]
fn list_from_the_same_list_in_a_loop_releases_each_copy() {
    assert_runs_with_output(
        "
use system.collections.list

fn main()
    let src = List([\"PEAR\".to_lower(), \"APPLE\".to_lower()])
    var total = 0
    var i = 0
    while i < 20
        var l = List(src)
        l.push(\"FIG\".to_lower())
        total = total + l.length()
        i = i + 1
    println(f\"{total}|{src[0]}|{src.length()}\")
",
        "60|pear|2",
    );
}

#[test]
fn list_from_list_parameter_of_a_generic_function_copies_at_every_instantiation() {
    assert_heap_guard_output(
        "
use system.collections.list

fn copy_of<T>(l [T]) [T]
    return List(l)

fn main()
    let ints = List([1, 2, 3])
    var ic = copy_of(ints)
    ic.push(4)
    let strs = List([\"PEAR\".to_lower(), \"APPLE\".to_lower()])
    var sc = copy_of(strs)
    sc.push(\"FIG\".to_lower())
    println(f\"{ints.length()}|{ic[2]}|{ic[3]}|{strs.length()}|{sc[0]}|{sc[2]}\")
",
        "3|3|4|2|pear|fig",
    );
}

#[test]
fn list_with_type_argument_copies_a_matching_array() {
    assert_runs_with_output(
        "
use system.collections.list

fn main()
    var l = List<int>([1, 2, 3])
    l.push(4)
    println(f\"{l.length()}|{l[0]}|{l[3]}\")
",
        "4|1|4",
    );
}

#[test]
fn list_with_type_argument_copies_a_matching_list() {
    assert_runs_with_output(
        "
use system.collections.list

fn main()
    let src = List([1, 2, 3])
    var copy = List<int>(src)
    copy.push(4)
    println(f\"{src.length()}|{copy.length()}|{copy[2]}\")
",
        "3|4|3",
    );
}

#[test]
fn list_with_type_argument_copies_a_matching_list_of_strings() {
    assert_runs_with_output(
        "
use system.collections.list

fn main()
    let src = List([\"PEAR\".to_lower(), \"APPLE\".to_lower()])
    var copy = List<String>(src)
    println(f\"{copy[0]}|{copy[1]}|{copy.length()}\")
",
        "pear|apple|2",
    );
}

#[test]
fn list_with_type_argument_accepts_an_empty_array_literal() {
    assert_runs_with_output(
        "
use system.collections.list

fn main()
    var l = List<int>([])
    l.push(7)
    println(f\"{l.length()}|{l[0]}\")
",
        "1|7",
    );
}

#[test]
fn list_with_a_generic_parameter_as_its_type_argument_copies_at_every_instantiation() {
    // Inside a generic body the written element type is the parameter itself,
    // so the argument check must accept a sequence of that same parameter.
    assert_runs_with_output(
        "
use system.collections.list

fn copy_of<T>(l [T]) [T]
    return List<T>(l)

fn main()
    let ints = List([1, 2, 3])
    var ic = copy_of(ints)
    ic.push(4)
    let strs = List([\"PEAR\".to_lower()])
    var sc = copy_of(strs)
    println(f\"{ints.length()}|{ic.length()}|{ic[3]}|{sc[0]}\")
",
        "3|4|4|pear",
    );
}

#[test]
fn list_with_type_argument_copies_a_matching_narrow_element_width() {
    assert_runs_with_output(
        "
use system.collections.list

fn main()
    let src = List<u8>([200, 7])
    let copy = List<u8>(src)
    println(f\"{copy[0]}|{copy[1]}\")
",
        "200|7",
    );
}
