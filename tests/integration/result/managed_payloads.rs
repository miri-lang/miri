// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A `Result` whose payloads are managed is itself a managed value wherever it
//! is held — a local, a class field, a list element — and is released exactly
//! once. Built from computed strings, so a missed or doubled release is not
//! masked by an immortal literal.

use super::utils::*;

#[test]
fn test_a_result_of_strings_held_in_a_local_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.io

fn make(ok bool, s String) Result<String, String>
    if ok
        return Result.Ok(f"{s}-ok")
    return Result.Err(f"{s}-err")

fn main()
    let s = "v"
    let r = make(true, s)
    let e = make(false, s)
    match r
        Result.Ok(v): println(v)
        Result.Err(x): println(x)
    match e
        Result.Ok(v): println(v)
        Result.Err(x): println(x)
"#,
        "v-ok\nv-err",
    );
}

#[test]
fn test_a_result_of_strings_held_in_a_class_field_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.io

class Holder
    var r Result<String, String>

fn main()
    let s = "f"
    let h = Holder(r: Result.Ok(f"{s}-field"))
    match h.r
        Result.Ok(v): println(v)
        Result.Err(x): println(x)
"#,
        "f-field",
    );
}

#[test]
fn test_a_result_of_strings_held_in_a_list_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

fn main()
    let s = "l"
    let rs = [Result.Ok(f"{s}-0"), Result.Err(f"{s}-1")]
    var list List<Result<String, String>> = List(rs)
    for r in list
        match r
            Result.Ok(v): println(v)
            Result.Err(x): println(x)
"#,
        "l-0\nl-1",
    );
}
