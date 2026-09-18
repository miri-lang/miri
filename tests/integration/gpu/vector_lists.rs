// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Collections of vectors (`Vec2`/`Vec3`/`Vec4`) store each element inline, as
//! its components laid out at the std430 stride. Every way an element enters a
//! collection — `push`, `insert`, an array literal, `List([...])` — must write
//! the components themselves, never the address of the vector they came from,
//! and every index read must find them again.

use super::utils::*;

#[test]
fn list_of_vec3_filled_by_push_reads_back_every_component() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List<Vec3<f32>>()
    vs.push(Vec3<f32>(1.0, 2.0, 3.0))
    vs.push(Vec3<f32>(4.0, 5.0, 6.0))
    println(f'{vs[0].x} {vs[0].y} {vs[0].z} | {vs[1].x} {vs[1].y} {vs[1].z} | {vs.length()}')
";
    assert_runs_with_output(source, "1.0 2.0 3.0 | 4.0 5.0 6.0 | 2");
}

#[test]
fn list_of_vec2_and_vec4_filled_by_push_reads_back_every_component() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var twos = List<Vec2<i32>>()
    twos.push(Vec2<i32>(7, 8))
    twos.push(Vec2<i32>(9, 10))
    var fours = List<Vec4<f32>>()
    fours.push(Vec4<f32>(1.0, 2.0, 3.0, 4.0))
    fours.push(Vec4<f32>(5.0, 6.0, 7.0, 8.0))
    println(f'{twos[0].x} {twos[0].y} {twos[1].x} {twos[1].y}')
    println(f'{fours[0].x} {fours[0].w} {fours[1].x} {fours[1].z} {fours[1].w}')
";
    assert_runs_with_output(source, "7 8 9 10\n1.0 4.0 5.0 7.0 8.0");
}

#[test]
fn list_of_vectors_with_64_bit_components_filled_by_push_reads_back_every_component() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List<Vec3<f64>>()
    vs.push(Vec3<f64>(1.5, 2.5, 3.5))
    vs.push(Vec3<f64>(4.5, 5.5, 6.5))
    println(f'{vs[0].x} {vs[0].y} {vs[0].z} | {vs[1].x} {vs[1].y} {vs[1].z}')
";
    assert_runs_with_output(source, "1.5 2.5 3.5 | 4.5 5.5 6.5");
}

#[test]
fn list_of_vectors_with_inferred_component_width_filled_by_push_reads_back_every_component() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var floats = List<Vec3<float>>()
    floats.push(Vec3(1.0, 2.0, 3.0))
    floats.push(Vec3(4.0, 5.0, 6.0))
    var ints = List<Vec2<int>>()
    ints.push(Vec2(11, 12))
    ints.push(Vec2(13, 14))
    println(f'{floats[0].x} {floats[0].y} {floats[0].z} | {floats[1].x} {floats[1].y} {floats[1].z}')
    println(f'{ints[0].x} {ints[0].y} {ints[1].x} {ints[1].y}')
";
    assert_runs_with_output(source, "1.0 2.0 3.0 | 4.0 5.0 6.0\n11 12 13 14");
}

#[test]
fn array_literal_of_vectors_with_inferred_component_width_reads_back_every_component() {
    let source = "
use system.gpu.vector

fn main()
    let floats = [Vec3(1.0, 2.0, 3.0), Vec3(4.0, 5.0, 6.0)]
    let ints = [Vec2(1, 2), Vec2(3, 4)]
    println(f'{floats[0].x} {floats[0].z} {floats[1].x} {floats[1].z} | {ints[0].x} {ints[0].y} {ints[1].x} {ints[1].y}')
";
    assert_runs_with_output(source, "1.0 3.0 4.0 6.0 | 1 2 3 4");
}

#[test]
fn list_of_vectors_built_from_an_array_literal_reads_back_every_component() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    let twos = List<Vec2<f32>>([Vec2<f32>(1.0, 2.0), Vec2<f32>(3.0, 4.0)])
    let threes = List([Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)])
    let inferred = List([Vec3(7.0, 8.0, 9.0), Vec3(10.0, 11.0, 12.0)])
    println(f'{twos[0].x} {twos[0].y} {twos[1].x} {twos[1].y}')
    println(f'{threes[0].x} {threes[0].z} {threes[1].x} {threes[1].z}')
    println(f'{inferred[0].x} {inferred[0].z} {inferred[1].x} {inferred[1].z}')
";
    assert_runs_with_output(
        source,
        "1.0 2.0 3.0 4.0\n1.0 3.0 4.0 6.0\n7.0 9.0 10.0 12.0",
    );
}

#[test]
fn list_of_vectors_grown_after_a_literal_keeps_every_element() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List([Vec3<f32>(1.0, 2.0, 3.0)])
    vs.push(Vec3<f32>(4.0, 5.0, 6.0))
    vs.push(Vec3<f32>(7.0, 8.0, 9.0))
    println(f'{vs[0].z} {vs[1].y} {vs[2].x} {vs.length()}')
";
    assert_runs_with_output(source, "3.0 5.0 7.0 3");
}

#[test]
fn list_of_vectors_insert_shifts_later_elements_and_stores_components() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List<Vec3<f32>>()
    vs.push(Vec3<f32>(1.0, 2.0, 3.0))
    vs.push(Vec3<f32>(7.0, 8.0, 9.0))
    vs.insert(1, Vec3<f32>(4.0, 5.0, 6.0))
    for v in vs
        println(f'{v.x} {v.y} {v.z}')
";
    assert_runs_with_output(source, "1.0 2.0 3.0\n4.0 5.0 6.0\n7.0 8.0 9.0");
}

#[test]
fn vector_pushed_from_a_local_is_copied_and_the_local_stays_usable() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var v = Vec3<f32>(1.0, 2.0, 3.0)
    var vs = List<Vec3<f32>>()
    vs.push(v)
    v.x = 9.0
    vs.push(v)
    println(f'{vs[0].x} {vs[1].x} {v.x} {v.y}')
";
    assert_runs_with_output(source, "1.0 9.0 9.0 2.0");
}

#[test]
fn popping_a_vector_off_a_list_reads_back_every_component() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List([Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)])
    match vs.pop()
        Some(v): println(f'{v.x} {v.y} {v.z} {vs.length()}')
        None: println('none')
";
    assert_runs_with_output(source, "4.0 5.0 6.0 1");
}

#[test]
fn a_popped_vector_survives_the_list_reusing_the_slot_it_came_from() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List([Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)])
    match vs.pop()
        Some(v)
            vs.clear()
            vs.push(Vec3<f32>(7.0, 8.0, 9.0))
            vs.push(Vec3<f32>(10.0, 11.0, 12.0))
            println(f'{v.x} {v.y} {v.z}')
        None
            println('none')
";
    assert_runs_with_output(source, "4.0 5.0 6.0");
}

#[test]
fn removing_a_vector_at_an_index_reads_back_every_component_and_shifts_the_rest() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List([Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0), Vec3<f32>(7.0, 8.0, 9.0)])
    match vs.remove_at(0)
        Some(v): println(f'{v.x} {v.y} {v.z}')
        None: println('none')
    println(f'{vs[0].x} {vs[1].x} {vs.length()}')
";
    assert_runs_with_output(source, "1.0 2.0 3.0\n4.0 7.0 2");
}

#[test]
fn popping_a_vec2_and_a_vec4_off_a_list_reads_back_every_component() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var twos = List([Vec2<i32>(7, 8), Vec2<i32>(9, 10)])
    match twos.pop()
        Some(t): println(f'{t.x} {t.y}')
        None: println('none')
    var fours = List([Vec4<f32>(1.0, 2.0, 3.0, 4.0), Vec4<f32>(5.0, 6.0, 7.0, 8.0)])
    match fours.remove_at(1)
        Some(q): println(f'{q.x} {q.y} {q.z} {q.w}')
        None: println('none')
";
    assert_runs_with_output(source, "9 10\n5.0 6.0 7.0 8.0");
}

#[test]
fn popping_every_vector_off_a_list_then_popping_again_reports_none() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List([Vec2<f32>(1.0, 2.0)])
    match vs.pop()
        Some(v): println(f'{v.x} {v.y}')
        None: println('none')
    match vs.pop()
        Some(v): println(f'{v.x} {v.y}')
        None: println('none')
    match vs.remove_at(0)
        Some(v): println(f'{v.x} {v.y}')
        None: println('none')
";
    assert_runs_with_output(source, "1.0 2.0\nnone\nnone");
}

#[test]
fn removing_a_vector_at_an_index_the_list_does_not_hold_reports_none() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List([Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)])
    var negative = 0
    negative = negative - 1
    match vs.remove_at(negative)
        Some(v): println(f'{v.y}')
        None: println('negative none')
    match vs.remove_at(9)
        Some(v): println(f'{v.y}')
        None: println('past-end none')
    println(f'{vs.length()}')
";
    assert_runs_with_output(source, "negative none\npast-end none\n2");
}
