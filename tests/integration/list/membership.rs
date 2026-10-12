// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `x in l` on a list is the call `l.contains(x)`: it compares elements by
//! their `equals`, and is refused wherever that call is.

use super::utils::*;

#[test]
fn membership_in_a_list_of_ints_answers_by_value() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<int>()
    l.push(3)
    println(f"{3 in l} {7 in l}")
"#,
        "true false",
    );
}

#[test]
fn membership_in_a_list_of_strings_compares_their_text() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    var l = List<String>()
    l.push("a" + "b")
    let probe = "a" + "b"
    println(f"{probe in l} {'ba' in l}")
"#,
        "true false",
    );
}

#[test]
fn membership_in_a_list_of_instances_uses_their_equals() {
    assert_heap_guard_output(
        r#"
use system.collections.list

class Dog
    name String
    public fn equals(other Dog) bool
        return self.name == other.name
    public fn hash() int
        return 0

fn main()
    var l = List<Dog>()
    l.push(Dog(name: "rex"))
    println(f"{Dog(name: 'rex') in l} {Dog(name: 'max') in l}")
"#,
        "true false",
    );
}

#[test]
fn membership_in_an_empty_list_is_false() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let l = List<int>()
    println(f"{1 in l}")
"#,
        "false",
    );
}
