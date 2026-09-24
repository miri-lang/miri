// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reading an element back out of a set, at every element width: the set hands
//! the element's bytes to the caller's slot, so an element narrower or wider
//! than a value word, or a float, arrives whole and at its own width.

use crate::integration::utils::*;

#[test]
fn test_set_u8_iteration_reads_each_element() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<u8>()
    s.add(7)
    s.add(200)
    var total = 0
    for x in s
        total = total + (x as int)
    println(f"{total}")
"#,
        "207",
    );
}

#[test]
fn test_set_i16_iteration_keeps_the_sign() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<i16>()
    s.add(-300)
    var total = 0
    for x in s
        total = total + (x as int)
    println(f"{total}")
"#,
        "-300",
    );
}

#[test]
fn test_set_i128_iteration_reads_the_whole_element() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let big i128 = -18446744073709551621
    var s = Set<i128>()
    s.add(big)
    for x in s
        println(f"{x} {x == big}")
"#,
        "-18446744073709551621 true",
    );
}

#[test]
fn test_set_f32_iteration_reads_the_float() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let half f32 = 1.5
    var s = Set<f32>()
    s.add(half)
    for x in s
        println(f"{x == half}")
"#,
        "true",
    );
}

#[test]
fn test_sets_of_two_element_widths_share_one_module() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var narrow = Set<u8>()
    narrow.add(9)
    var wide = Set<int>()
    wide.add(1000)
    var total = 0
    for x in narrow
        total = total + (x as int)
    for y in wide
        total = total + y
    println(f"{total}")
"#,
        "1009",
    );
}

#[test]
fn test_set_of_vec3_finds_and_reads_back_its_elements() {
    assert_runs_with_output(
        r#"
use system.gpu.vector
use system.collections.set

fn main()
    let a = Vec3<f32>(1.0, 2.0, 3.0)
    var s = Set<Vec3<f32>>()
    s.add(a)
    s.add(Vec3<f32>(4.0, 5.0, 6.0))
    var total f32 = 0.0
    for v in s
        total = total + v.x + v.y + v.z
    println(f"{s.contains(a)} {s.contains(Vec3<f32>(9.0, 9.0, 9.0))} {total == 21.0}")
"#,
        "true false true",
    );
}

#[test]
fn test_set_i128_iteration_keeps_elements_that_share_a_low_word_apart() {
    // `i128::MAX` and `-1` fill their low eight bytes with the same ones, so an
    // element read back as a value word makes the two look equal.
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let big i128 = 170141183460469231731687303715884105727
    let twin i128 = -1
    var s = Set<i128>()
    s.add(big)
    s.add(twin)
    var bigs = 0
    var twins = 0
    for x in s
        if x == big: bigs = bigs + 1
        if x == twin: twins = twins + 1
    println(f"{bigs} {twins}")
"#,
        "1 1",
    );
}

#[test]
fn test_set_of_vec2_contains_and_remove_agree_with_add() {
    assert_runs_with_output(
        r#"
use system.collections.set
use system.gpu.vector

fn main()
    let a = Vec2<f32>(1.0, 2.0)
    var s = Set<Vec2<f32>>()
    s.add(a)
    let found = s.contains(a)
    let removed = s.remove(Vec2<f32>(1.0, 2.0))
    println(f"{found} {removed} {s.length()}")
"#,
        "true true 0",
    );
}

#[test]
fn test_sets_of_vec3_and_vec4_deduplicate_by_value() {
    assert_runs_with_output(
        r#"
use system.collections.set
use system.gpu.vector

fn main()
    let b = Vec3<f32>(1.0, 2.0, 3.0)
    var three = Set<Vec3<f32>>()
    three.add(b)
    three.add(Vec3<f32>(1.0, 2.0, 3.0))
    var four = Set<Vec4<f32>>()
    four.add(Vec4<f32>(1.0, 2.0, 3.0, 4.0))
    four.add(Vec4<f32>(1.0, 2.0, 3.0, 4.0))
    println(f"{three.length()} {four.length()} {three.contains(b)}")
"#,
        "1 1 true",
    );
}
