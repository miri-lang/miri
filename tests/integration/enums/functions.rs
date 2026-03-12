// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_enum_match_in_function() {
    assert_runs_with_output(
        r#"
use system.io

enum Direction
    North
    South
    East
    West

fn direction_code(d Direction) int
    match d
        Direction.North: 0
        Direction.South: 1
        Direction.East: 2
        Direction.West: 3

fn main()
    println(f"{direction_code(Direction.North)}")
    println(f"{direction_code(Direction.West)}")
"#,
        "0",
    );
}

#[test]
fn test_enum_param_match_all_variants() {
    assert_runs_with_output(
        r#"
use system.io

enum Signal
    Red
    Yellow
    Green

fn signal_wait(s Signal) int
    match s
        Signal.Red: 3
        Signal.Yellow: 1
        Signal.Green: 0

fn main()
    let r = signal_wait(Signal.Red)
    let y = signal_wait(Signal.Yellow)
    let g = signal_wait(Signal.Green)
    println(f"{r}")
    println(f"{y}")
    println(f"{g}")
"#,
        "3",
    );
}

#[test]
fn test_enum_match_returns_enum() {
    assert_runs_with_output(
        r#"
use system.io

enum Toggle
    On
    Off

fn flip(t Toggle) Toggle
    match t
        Toggle.On: Toggle.Off
        Toggle.Off: Toggle.On

fn main()
    let t1 = flip(Toggle.On)
    let t2 = flip(Toggle.Off)
    let r1 = match t1
        Toggle.On: 1
        Toggle.Off: 0
    let r2 = match t2
        Toggle.On: 1
        Toggle.Off: 0
    println(f"{r1}")
    println(f"{r2}")
"#,
        "0",
    );
}
