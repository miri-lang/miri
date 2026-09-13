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
