// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A program's own type that happens to share a name with a compiler-known
//! vector (`Vec2`, `Vec3`, `Vec4`) is an ordinary struct, and the program's own
//! functions named like the vector builtins (`dot`, `normalize`, …) are
//! ordinary functions on it. Only a vector written with its component
//! (`Vec3<f32>`) reaches the builtins.

use super::utils::*;

#[test]
fn test_a_users_vec3_and_its_own_normalize_run_on_the_cpu() {
    assert_runs_with_output(
        r#"
use system.io
use system.math

struct Vec3
    x float
    y float
    z float

fn dot(a Vec3, b Vec3) float: a.x * b.x + a.y * b.y + a.z * b.z

fn scale(a Vec3, k float) Vec3: Vec3(x: a.x * k, y: a.y * k, z: a.z * k)

fn normalize(a Vec3) Vec3: scale(a, 1.0 / sqrt(dot(a, a)))

fn length(a Vec3) float: sqrt(dot(a, a))

let v = normalize(Vec3(x: 0.0, y: 0.0, z: 4.0))
println(f"{v.x} {v.z} {length(Vec3(x: 0.0, y: 6.0, z: 8.0))}")
"#,
        "0.0 1.0 10.0",
    );
}

#[test]
fn test_a_users_vec2_keeps_its_own_dot() {
    assert_runs_with_output(
        r#"
use system.io

struct Vec2
    x int
    y int

fn dot(a Vec2, b Vec2) int: a.x * b.x + a.y * b.y

println(f"{dot(Vec2(x: 1, y: 2), Vec2(x: 3, y: 4))}")
"#,
        "11",
    );
}
