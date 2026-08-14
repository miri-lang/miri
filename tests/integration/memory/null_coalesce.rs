// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Tests for `??` over an Option carrying a managed payload. Reading the payload
// out of the Some branch retains it, so the Option the operator consumed has to
// be released once every branch has read from it — otherwise every evaluation
// that takes the Some branch strands one allocation.

use super::super::utils::*;

#[test]
fn test_coalesce_over_call_result_no_leak() {
    assert_runs_with_output(
        r#"
fn labelled(prefix String) String?
    let text = prefix + "!"
    Some(text)

fn main()
    let value = labelled("a") ?? ""
    println(value)
"#,
        "a!",
    );
}

#[test]
fn test_coalesce_over_named_local_no_leak() {
    assert_runs_with_output(
        r#"
fn labelled(prefix String) String?
    let text = prefix + "!"
    Some(text)

fn main()
    let candidate = labelled("b")
    let value = candidate ?? ""
    println(value)
"#,
        "b!",
    );
}

#[test]
fn test_coalesce_none_branch_no_leak() {
    assert_runs_with_output(
        r#"
fn labelled(prefix String) String?
    None

fn main()
    let value = labelled("c") ?? "fallback"
    println(value)
"#,
        "fallback",
    );
}

#[test]
fn test_coalesce_in_loop_no_leak() {
    assert_runs_with_output(
        r#"
fn labelled(prefix String) String?
    let text = prefix + "!"
    Some(text)

fn main()
    var total = 0
    var index = 0
    while index < 50
        let value = labelled("d") ?? ""
        total = total + value.length()
        index = index + 1
    println(f"{total}")
"#,
        "100",
    );
}

#[test]
fn test_coalesce_as_call_argument_no_leak() {
    assert_runs_with_output(
        r#"
fn labelled(prefix String) String?
    let text = prefix + "!"
    Some(text)

fn main()
    println(labelled("e") ?? "")
"#,
        "e!",
    );
}

#[test]
fn test_coalesce_over_list_payload_no_leak() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn digits(count int) [int]?
    var items = List<int>()
    var index = 0
    while index < count
        items.push(index)
        index = index + 1
    Some(items)

fn main()
    let items = digits(3) ?? List<int>()
    println(f"{items.length()}")
"#,
        "3",
    );
}

#[test]
fn test_coalesce_on_string_from_code_point_no_leak() {
    assert_runs_with_output(
        r#"
use system.string

fn main()
    let character = String.from_code_point(104) ?? ""
    println(character)
"#,
        "h",
    );
}
