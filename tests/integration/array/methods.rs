// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_array_baselist_methods() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [10, 20, 30]
println(f"{a.first() ?? -1}")
println(f"{a.last() ?? -1}")
println(f"{a.is_empty()}")
println(f"{a.contains(20)}")
println(f"{a.index_of(30) ?? -1}")
"#,
        "10\n30\nfalse\ntrue\n2",
    );
}

#[test]
fn test_array_reverse() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [10000000000, 20000000000, 30000000000]
a.reverse()
println(f"{a[0]} {a[1]} {a[2]}")
"#,
        "30000000000 20000000000 10000000000",
    );
}

/// A struct field passed to `set` is stored at the field's own type. Reading it
/// back at the struct's type instead counts a plain integer as a pointer, so the
/// untouched neighbour proves the write landed in one slot at the right width.
#[test]
fn test_array_set_projected_field() {
    assert_runs_with_output(
        r#"
use system.collections.array

struct Reading
    amount int

fn main()
    let r = Reading(100)
    var a = [0, 0]
    a.set(0, r.amount)
    println(f"{a[0]} {a[1]}")
    println(f"{r.amount}")
"#,
        "100 0\n100",
    );
}

#[test]
fn test_array_set_method() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [10, 20, 30]
a.set(1, 99)
println(f"{a[0]} {a[1]} {a[2]}")
"#,
        "10 99 30",
    );
}

#[test]
fn test_array_length_method() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [1, 2, 3, 4, 5]
println(f"{a.length()}")
"#,
        "5",
    );
}

#[test]
fn test_array_element_at() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [10, 20, 30]
println(f"{a.element_at(1)}")
"#,
        "20",
    );
}

#[test]
fn test_array_is_empty_false() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [1]
println(f"{a.is_empty()}")
"#,
        "false",
    );
}

#[test]
fn test_array_first_last_methods() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [100, 200, 300]
println(f"{a.first() ?? -1}")
println(f"{a.last() ?? -1}")
"#,
        "100\n300",
    );
}

#[test]
fn test_array_contains_true_and_false() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [5, 10, 15]
println(f"{a.contains(10)}")
println(f"{a.contains(99)}")
"#,
        "true\nfalse",
    );
}

#[test]
fn test_array_index_of_found_and_not_found() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [5, 10, 15]
println(f"{a.index_of(15) ?? -1}")
match a.index_of(99)
    Some(_)
        println("found")
    None
        println("not_found")
"#,
        "2\nnot_found",
    );
}

#[test]
fn test_array_reverse_two_elements() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [1, 2]
a.reverse()
println(f"{a[0]} {a[1]}")
"#,
        "2 1",
    );
}

#[test]
fn test_array_sort() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [30, 10, 20, 5]
a.sort()
println(f"{a[0]} {a[1]} {a[2]} {a[3]}")
"#,
        "5 10 20 30",
    );
}

#[test]
fn test_array_sort_already_sorted() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [1, 2, 3]
a.sort()
println(f"{a[0]} {a[1]} {a[2]}")
"#,
        "1 2 3",
    );
}

#[test]
fn test_array_sort_reverse_order() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [5, 4, 3, 2, 1]
a.sort()
println(f"{a[0]} {a[1]} {a[2]} {a[3]} {a[4]}")
"#,
        "1 2 3 4 5",
    );
}

#[test]
fn test_array_index_of_duplicates() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [10, 20, 10, 30]
println(f"{a.index_of(10) ?? -1}")
"#,
        "0",
    );
}

#[test]
fn test_array_sort_with_duplicates_and_negatives() {
    assert_runs_with_output(
        r#"
use system.collections.array

let a = [5, -1, 5, 0, -5]
a.sort()
println(f"{a[0]} {a[1]} {a[2]} {a[3]} {a[4]}")
"#,
        "-5 -1 0 5 5",
    );
}
