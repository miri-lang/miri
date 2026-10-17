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

#[test]
fn test_a_users_vec3_read_from_a_list_is_counted_like_any_struct() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

struct Vec3
    x float
    y float
    z float

fn first(xs List<Vec3>) Vec3
    let v = xs[0]
    v

fn main()
    var xs = List<Vec3>()
    xs.push(Vec3(x: 1.0, y: 2.0, z: 3.0))
    let v = first(xs)
    let w = List<Vec3>([Vec3(x: 7.0, y: 8.0, z: 9.0)])
    var total = 0.0
    for p in w
        total = total + p.y
    println(f"{v.x} {v.z} {xs[0].y} {total}")
"#,
        "1.0 3.0 2.0 8.0",
    );
}

#[test]
fn test_a_users_vec2_class_in_a_list_is_released_once() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list

class Vec2
    var x int
    fn drop()
        println(f"drop {self.x}")

fn main()
    var xs = List<Vec2>()
    xs.push(Vec2(x: 1))
    xs.push(Vec2(x: 2))
    let a = xs[0]
    println(f"got {a.x}")
"#,
        "got 1\ndrop 1\ndrop 2",
    );
}

#[test]
fn test_a_users_vec3_is_matched_and_copied_by_value_like_any_struct() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list
use system.collections.set

struct Vec3
    x float
    y float
    z float

fn main()
    var s = Set<Vec3>()
    s.add(Vec3(x: 1.0, y: 2.0, z: 3.0))
    s.add(Vec3(x: 1.0, y: 2.0, z: 3.0))
    s.add(Vec3(x: 4.0, y: 2.0, z: 3.0))
    var xs = List<Vec3>()
    xs.push(Vec3(x: 1.0, y: 2.0, z: 3.0))
    var ys = xs
    xs[0].x = 9.0
    println(f"{s.length()} {xs[0].x} {ys[0].x}")
"#,
        "2 9.0 1.0",
    );
}
