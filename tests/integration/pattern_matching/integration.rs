// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_match_nested() {
    assert_runs_with_output(
        r#"
use system.io

let outer = 1
let inner = 2
let result = match outer
    1: match inner
        1: 10
        2: 20
        _: 30
    _: 0
print(f"{result}")
"#,
        "20",
    );
}

#[test]
fn test_match_used_as_statement() {
    assert_runs_with_output(
        r#"
use system.io

var count = 0
let x = 2
let delta = match x
    1: 10
    2: 20
    _: 99
count = count + delta
print(f"{count}")
"#,
        "20",
    );
}

#[test]
fn test_match_in_function() {
    assert_runs_with_output(
        r#"
use system.io

fn describe(n int) int
    match n
        1: 100
        2: 200
        3: 300
        _: 0

fn main()
    println(f"{describe(1)}")
    println(f"{describe(2)}")
    println(f"{describe(3)}")
    println(f"{describe(99)}")
"#,
        "100\n200\n300\n0",
    );
}

#[test]
fn test_match_function_fibonacci() {
    assert_runs_with_output(
        r#"
use system.io

fn fib(n int) int
    match n
        0: 0
        1: 1
        _: fib(n - 1) + fib(n - 2)

fn main()
    println(f"{fib(0)}")
    println(f"{fib(1)}")
    println(f"{fib(5)}")
    println(f"{fib(7)}")
"#,
        "0\n1\n5\n13",
    );
}

#[test]
fn test_match_without_default_hits_arm() {
    assert_runs_with_output(
        r#"
use system.io

let x = 1
let result = match x
    1: 10
    2: 20
print(f"{result}")
"#,
        "10",
    );
}

#[test]
fn test_match_collection_in_enum_arm() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

enum Result
    Ok(List<int>)
    Err(String)

fn main()
    let r = Result.Ok(List([1, 2, 3]))
    
    let result = match r
        Result.Ok(l): l.length()
        Result.Err(e): 0
        
    println(f"{result}")
"#,
        "3",
    );
}
