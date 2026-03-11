// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_nested_options() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let opt Option<Option<int>> = Some(Some(42))
    match opt
        Some(inner): match inner
            Some(v): println(f"Double Some: {v}")
            None: println("Inner None")
        None: println("Outer None")

    let opt2 Option<Option<int>> = Some(None)
    match opt2
        Some(inner): match inner
            Some(v): println(f"Double Some: {v}")
            None: println("Inner None")
        None: println("Outer None")
"#,
        "Double Some: 42\nInner None",
    );
}

#[test]
fn test_options_in_collections() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

fn main()
    let l List<int?> = List([Some(1), None, Some(3)])
    var i = 0
    while i < l.length()
        match l.get(i)
            Some(v): println(f"Value: {v}")
            None: println("None")
        i = i + 1
"#,
        "Value: 1\nNone\nValue: 3",
    );
}

#[test]
fn test_option_equality() {
    assert_runs_with_output(
        r#"
use system.io

fn main()
    let a int? = Some(1)
    let b int? = Some(1)
    let c int? = Some(2)
    let d int? = None
    let e int? = None

    if a == b
        println("a == b")
    if a != c
        println("a != c")
    if a != d
        println("a != d")
    if d == e
        println("d == e")
"#,
        "a == b\na != c\na != d\nd == e",
    );
}

#[test]
fn test_function_returning_option_early_return() {
    assert_runs_with_output(
        r#"
use system.io

fn get_value(condition bool) String?
    if condition
        return None
    return Some("Success")

fn main()
    let a = get_value(true)
    let b = get_value(false)
    
    match a
        Some(s): println(f"A: {s}")
        None: println("A: None")

    match b
        Some(s): println(f"B: {s}")
        None: println("B: None")
"#,
        "A: None\nB: Success",
    );
}
