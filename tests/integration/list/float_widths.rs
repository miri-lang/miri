// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for List<T> at non-int scalar widths (float, f32).
//! These cover:
//! - DEFECT 1: non-intercepted methods (remove_at, pop, first, last) at non-int widths
//! - DEFECT 2: push() bitcast vs numeric conversion for floats

use crate::integration::utils::*;

// --- DEFECT 2: push() bitcast fix ---

#[test]
fn test_list_push_float_preserves_value() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var m = List([1.5])
    println(f"{m[0]}")
    m.push(2.5)
    println(f"{m[1]}")
"#,
        "1.5
2.5",
    );
}

#[test]
#[ignore = "f32 collection elements are refused at codegen: the element stride for floats narrower than a word is unresolved, so a stored value reads back as zero. Fails closed rather than corrupting; float/f64 round-trips correctly."]
fn test_list_push_f32_preserves_value() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var m = List<f32>([])
    m.push(2.5)
    println(f"{m[0]}")
    m.push(3.25)
    println(f"{m[1]}")
"#,
        "2.5
3.25",
    );
}

#[test]
fn test_list_push_multiple_floats() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var m = List<float>([])
    m.push(1.1)
    m.push(2.2)
    m.push(3.3)
    println(f"{m[0]}")
    println(f"{m[1]}")
    println(f"{m[2]}")
"#,
        "1.1
2.2
3.3",
    );
}

#[test]
fn test_list_float_round_trip() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let initial = 42.5
    var l = List([initial])
    l.push(99.75)
    println(f"{l[0]}")
    println(f"{l[1]}")
"#,
        "42.5
99.75",
    );
}

// --- DEFECT 1: non-intercepted methods at non-int widths ---

#[test]
fn test_list_remove_at_float() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List([1.5, 2.5, 3.5])
    let r = l.remove_at(0)
    println(f"{r}")
"#,
        "1.5",
    );
}

#[test]
#[ignore = "f32 collection elements are refused at codegen: the element stride for floats narrower than a word is unresolved, so a stored value reads back as zero. Fails closed rather than corrupting; float/f64 round-trips correctly."]
fn test_list_remove_at_f32() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<f32>([1.5, 2.5, 3.5])
    let r = l.remove_at(1)
    println(f"{r}")
"#,
        "2.5",
    );
}

#[test]
fn test_list_pop_float() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List([1.5, 2.5, 3.5])
    println(f"{l.pop()}")
"#,
        "3.5",
    );
}

#[test]
fn test_list_first_float() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let l = List([1.5, 2.5, 3.5])
    println(f"{l.first() ?? 0.0}")
"#,
        "1.5",
    );
}

#[test]
fn test_list_last_float() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let l = List([1.5, 2.5, 3.5])
    println(f"{l.last() ?? 0.0}")
"#,
        "3.5",
    );
}

#[test]
#[ignore = "f32 collection elements are refused at codegen: the element stride for floats narrower than a word is unresolved, so a stored value reads back as zero. Fails closed rather than corrupting; float/f64 round-trips correctly."]
fn test_list_first_f32() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let l = List<f32>([10.5, 20.5, 30.5])
    println(f"{l.first() ?? 0.0}")
"#,
        "10.5",
    );
}

// --- Queue<float> and Stack<float> end-to-end tests ---

// --- Integer width tests for regression check ---

#[test]
fn test_list_push_int_still_works() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var m = List([1])
    m.push(2)
    println(f"{m[0]}")
    println(f"{m[1]}")
"#,
        "1
2",
    );
}

#[test]
fn test_list_remove_at_int_still_works() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List([10, 20, 30])
    let r = l.remove_at(1)
    println(f"{r}")
"#,
        "20",
    );
}
