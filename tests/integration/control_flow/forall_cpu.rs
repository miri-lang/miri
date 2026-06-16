// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_forall_cpu_1d() {
    assert_runs_with_output(
        r#"
use system.io
forall i in 0..5
    print(f"{i}")
    "#,
        "01234",
    );
}

#[test]
fn test_forall_cpu_1d_inclusive() {
    assert_runs_with_output(
        r#"
use system.io
forall i in 1..=3
    print(f"{i}")
    "#,
        "123",
    );
}

#[test]
fn test_forall_cpu_2d() {
    assert_runs_with_output(
        r#"
use system.io
forall x, y in 0..2, 0..3
    print(f"{x}-{y},")
    "#,
        "0-0,0-1,0-2,1-0,1-1,1-2,",
    );
}

#[test]
fn test_forall_cpu_3d() {
    assert_runs_with_output(
        r#"
use system.io
forall x, y, z in 0..2, 0..2, 0..2
    print(f"({x},{y},{z})")
    "#,
        "(0,0,0)(0,0,1)(0,1,0)(0,1,1)(1,0,0)(1,0,1)(1,1,0)(1,1,1)",
    );
}

#[test]
fn test_forall_cpu_variable_bound() {
    assert_runs_with_output(
        r#"
use system.io
var n = 3
forall i in 0..n
    print(f"{i}")
    "#,
        "012",
    );
}

#[test]
fn test_forall_cpu_2d_variable_bounds() {
    assert_runs_with_output(
        r#"
use system.io
var a = 2
var b = 2
forall i, j in 0..a, 0..b
    print(f"{i}{j}")
    "#,
        "00011011",
    );
}

#[test]
fn test_forall_cpu_empty_range() {
    assert_runs_with_output(
        r#"
use system.io
forall i in 0..0
    print(f"{i}")
    "#,
        "",
    );
}

#[test]
fn test_forall_cpu_break() {
    assert_runs_with_output(
        r#"
use system.io
forall i in 0..5
    if i == 3
        break
    print(f"{i}")
    "#,
        "012",
    );
}

#[test]
fn test_forall_cpu_continue() {
    assert_runs_with_output(
        r#"
use system.io
forall i in 0..4
    if i == 1
        continue
    print(f"{i}")
    "#,
        "023",
    );
}

#[test]
fn test_forall_cpu_local_variable_per_iteration() {
    assert_runs_with_output(
        r#"
use system.io
forall i in 0..3
    var x = i + 10
    print(f"{x}")
    "#,
        "101112",
    );
}
