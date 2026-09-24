// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reading a key or a value back out of a map, at every width: the map hands
//! the bytes to the caller's slot, so a key or value narrower or wider than a
//! value word, or a float, arrives whole and at its own width.

use crate::integration::utils::*;

#[test]
fn test_map_f32_value_reads_back_through_index_and_get() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let half f32 = 1.5
    var m = Map<int, f32>()
    m.set(1, half)
    let viaget = m.get(1) ?? 0.0
    println(f"{m[1] == half} {viaget == half}")
"#,
        "true true",
    );
}

#[test]
fn test_map_float_value_reads_back_through_get() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<String, float>()
    m.set("a", 2.5)
    let v = m.get("a") ?? 0.0
    println(f"{v}")
"#,
        "2.5",
    );
}

#[test]
fn test_map_i128_value_reads_back_whole() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let big i128 = -18446744073709551621
    var m = Map<int, i128>()
    m.set(1, big)
    let viaget = m.get(1) ?? 0
    println(f"{m[1]} {m[1] == big} {viaget == big}")
"#,
        "-18446744073709551621 true true",
    );
}

#[test]
fn test_map_u8_key_iteration_reads_each_key() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var m = Map<u8, int>()
    m.set(3, 10)
    m.set(250, 20)
    var keys = 0
    var values = 0
    for k, v in m
        keys = keys + (k as int)
        values = values + v
    println(f"{keys} {values}")
"#,
        "253 30",
    );
}

#[test]
fn test_map_u128_value_iteration_reads_the_whole_value() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let big u128 = 18446744073709551623
    var m = Map<int, u128>()
    m.set(1, big)
    for k, v in m
        println(f"{k} {v} {v == big}")
"#,
        "1 18446744073709551623 true",
    );
}

#[test]
fn test_map_vec3_value_reads_back_through_index_and_get() {
    assert_runs_with_output(
        r#"
use system.gpu.vector
use system.collections.map

fn main()
    var m = Map<int, Vec3<f32>>()
    m.set(1, Vec3<f32>(1.0, 2.0, 3.0))
    let viaget = m.get(1) ?? Vec3<f32>(0.0, 0.0, 0.0)
    let direct = m[1]
    println(f"{viaget.z == 3.0} {direct.y == 2.0}")
"#,
        "true true",
    );
}

#[test]
fn test_map_float_value_reads_the_same_through_index_and_get() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    var f = Map<String, float>()
    f["x"] = 3.75
    let direct = f["x"]
    let viaget = f.get("x") ?? 0.0
    println(f"{direct} {viaget}")
"#,
        "3.75 3.75",
    );
}

#[test]
fn test_map_float_value_reads_back_inside_a_generic_body() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn lookup<K, V>(m Map<K, V>, key K, fallback V) V
    return m.get(key) ?? fallback

fn main()
    var m = Map<int, float>()
    m.set(1, 3.75)
    var n = Map<int, int>()
    n.set(1, 7)
    println(f"{lookup(m, 1, 0.0)} {lookup(n, 1, 0)}")
"#,
        "3.75 7",
    );
}

#[test]
fn test_map_i128_key_read_from_iteration_indexes_the_map() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let big i128 = 170141183460469231731687303715884105727
    var m = Map<i128, int>()
    m.set(big, 1)
    m.set(-1, 2)
    var total = 0
    for k in m
        total = total + m[k]
    println(f"{total}")
"#,
        "3",
    );
}

#[test]
fn test_maps_of_different_value_widths_share_one_function() {
    assert_runs_with_output(
        r#"
use system.collections.map
use system.collections.set

fn main()
    var a = Map<int, int>({})
    a.set(1, 10)
    var b = Map<i32, i32>({})
    b.set(1, 20)
    var c = Map<int, f32>({})
    c.set(1, 2.5)
    var s = Set<u8>()
    s.add(4)
    var e = 0
    for x in s
        e = x as int
    println(f"{a[1]} {b[1]} {c[1]} {e}")
"#,
        "10 20 2.5 4",
    );
}

#[test]
fn test_map_with_vec2_keys_finds_and_removes_the_key_it_stored() {
    assert_runs_with_output(
        r#"
use system.collections.map
use system.gpu.vector

fn main()
    let a = Vec2<f32>(1.0, 2.0)
    var m = Map<Vec2<f32>, int>()
    m.set(a, 5)
    let found = m.contains_key(Vec2<f32>(1.0, 2.0))
    let value = m[a]
    let removed = m.remove(a)
    println(f"{found} {value} {removed} {m.length()}")
"#,
        "true 5 true 0",
    );
}
