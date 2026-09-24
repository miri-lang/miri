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

#[test]
fn reading_the_first_and_last_vector_of_a_list_reads_back_every_component() {
    // `first` and `last` are written once over an opaque element type, so the
    // body reads one value word out of the slot — for a vector that is a prefix
    // of its components, and wrapping it as an optional hands back something
    // whose fields cannot be read.
    assert_runs_with_output(
        r#"
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List([Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)])
    match vs.first()
        Some(v): println(f"{v.x} {v.y} {v.z}")
        None: println("none")
    match vs.last()
        Some(v): println(f"{v.x} {v.y} {v.z}")
        None: println("none")
    println(f"{vs.length()}")
"#,
        "1.0 2.0 3.0
4.0 5.0 6.0
2",
    );
}

#[test]
fn reading_the_first_vector_does_not_remove_it() {
    // A peek leaves the list as it was: the element is copied out, not taken.
    assert_runs_with_output(
        r#"
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List([Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)])
    match vs.first()
        Some(v): println(f"{v.y}")
        None: println("none")
    match vs.first()
        Some(v): println(f"{v.y}")
        None: println("none")
    println(f"{vs.length()}")
"#,
        "2.0
2.0
2",
    );
}

#[test]
fn reading_the_first_and_last_vector_of_an_empty_list_reports_none() {
    assert_runs_with_output(
        r#"
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List<Vec3<f32>>()
    match vs.first()
        Some(v): println(f"{v.y}")
        None: println("first none")
    match vs.last()
        Some(v): println(f"{v.y}")
        None: println("last none")
"#,
        "first none
last none",
    );
}

#[test]
fn reading_the_first_and_last_vec2_and_vec4_reads_back_every_component() {
    assert_runs_with_output(
        r#"
use system.gpu.vector
use system.collections.list

fn main()
    var twos = List([Vec2<f32>(1.0, 2.0), Vec2<f32>(3.0, 4.0)])
    match twos.first()
        Some(v): println(f"{v.x} {v.y}")
        None: println("none")
    var fours = List([Vec4<f32>(1.0, 2.0, 3.0, 4.0), Vec4<f32>(5.0, 6.0, 7.0, 8.0)])
    match fours.last()
        Some(v): println(f"{v.x} {v.y} {v.z} {v.w}")
        None: println("none")
"#,
        "1.0 2.0
5.0 6.0 7.0 8.0",
    );
}

#[test]
fn a_single_element_list_reads_the_same_vector_as_first_and_last() {
    assert_runs_with_output(
        r#"
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List([Vec3<f32>(7.0, 8.0, 9.0)])
    match vs.first()
        Some(v): println(f"{v.y}")
        None: println("none")
    match vs.last()
        Some(v): println(f"{v.y}")
        None: println("none")
"#,
        "8.0
8.0",
    );
}

#[test]
fn searching_a_list_of_vectors_finds_the_element_that_matches_every_component() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    let vs = List([Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)])
    println(f'{vs.contains(Vec3<f32>(1.0, 2.0, 3.0))}')
    println(f'{vs.contains(Vec3<f32>(4.0, 5.0, 6.0))}')
    println(f'{vs.contains(Vec3<f32>(1.0, 2.0, 9.0))}')
    println(f'{vs.contains(Vec3<f32>(7.0, 8.0, 9.0))}')
";
    assert_runs_with_output(source, "true\ntrue\nfalse\nfalse");
}

#[test]
fn a_vector_that_matches_only_its_leading_components_is_not_found() {
    // The element is wider than one value word, so a search comparing only the
    // leading word would call these two vectors the same. The matching vector
    // is searched for beside it: answering `false` to both is what the search
    // did before it read past the first component, so the negative alone would
    // hold without the comparison reaching the trailing ones.
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    let vs = List([Vec3<f32>(1.0, 2.0, 3.0)])
    println(f'{vs.contains(Vec3<f32>(1.0, 2.0, 4.0))}')
    println(f'{vs.contains(Vec3<f32>(1.0, 2.0, 3.0))}')
    match vs.index_of(Vec3<f32>(1.0, 2.0, 4.0))
        Some(i): println(f'found at {i}')
        None: println('not found')
    match vs.index_of(Vec3<f32>(1.0, 2.0, 3.0))
        Some(i): println(f'found at {i}')
        None: println('not found')
";
    assert_runs_with_output(source, "false\ntrue\nnot found\nfound at 0");
}

#[test]
fn the_index_of_a_vector_is_the_first_slot_holding_it() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    let vs = List([Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0), Vec3<f32>(1.0, 2.0, 3.0)])
    match vs.index_of(Vec3<f32>(1.0, 2.0, 3.0))
        Some(i): println(f'{i}')
        None: println('none')
    match vs.index_of(Vec3<f32>(4.0, 5.0, 6.0))
        Some(i): println(f'{i}')
        None: println('none')
    match vs.index_of(Vec3<f32>(9.0, 9.0, 9.0))
        Some(i): println(f'{i}')
        None: println('none')
";
    assert_runs_with_output(source, "0\n1\nnone");
}

#[test]
fn searching_an_empty_list_of_vectors_finds_nothing() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    let vs = List<Vec3<f32>>()
    println(f'{vs.contains(Vec3<f32>(1.0, 2.0, 3.0))}')
    match vs.index_of(Vec3<f32>(1.0, 2.0, 3.0))
        Some(i): println(f'{i}')
        None: println('none')
";
    assert_runs_with_output(source, "false\nnone");
}

#[test]
fn removing_a_vector_by_value_takes_the_first_one_that_matches() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List([Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0), Vec3<f32>(1.0, 2.0, 3.0)])
    println(f'{vs.remove(Vec3<f32>(1.0, 2.0, 3.0))}')
    println(f'{vs.length()}')
    println(f'{vs[0].x} {vs[0].y} {vs[0].z} | {vs[1].x} {vs[1].y} {vs[1].z}')
    println(f'{vs.remove(Vec3<f32>(9.0, 9.0, 9.0))}')
    println(f'{vs.length()}')
";
    assert_runs_with_output(source, "true\n2\n4.0 5.0 6.0 | 1.0 2.0 3.0\nfalse\n2");
}

#[test]
fn searching_a_list_of_vec2_and_vec4_compares_every_component() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    let twos = List([Vec2<i32>(7, 8), Vec2<i32>(9, 10)])
    println(f'{twos.contains(Vec2<i32>(9, 10))}')
    println(f'{twos.contains(Vec2<i32>(9, 11))}')
    let fours = List([Vec4<f32>(1.0, 2.0, 3.0, 4.0), Vec4<f32>(5.0, 6.0, 7.0, 8.0)])
    println(f'{fours.contains(Vec4<f32>(5.0, 6.0, 7.0, 8.0))}')
    println(f'{fours.contains(Vec4<f32>(5.0, 6.0, 7.0, 9.0))}')
";
    assert_runs_with_output(source, "true\nfalse\ntrue\nfalse");
}

#[test]
fn searching_a_list_of_vectors_with_64_bit_components_compares_every_component() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    let vs = List([Vec3<f64>(1.5, 2.5, 3.5), Vec3<f64>(4.5, 5.5, 6.5)])
    println(f'{vs.contains(Vec3<f64>(4.5, 5.5, 6.5))}')
    println(f'{vs.contains(Vec3<f64>(4.5, 5.5, 9.5))}')
    match vs.index_of(Vec3<f64>(1.5, 2.5, 3.5))
        Some(i): println(f'{i}')
        None: println('none')
";
    assert_runs_with_output(source, "true\nfalse\n0");
}

#[test]
fn searching_a_list_of_vectors_leaves_it_unchanged() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    let vs = List([Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)])
    println(f'{vs.contains(Vec3<f32>(1.0, 2.0, 3.0))}')
    println(f'{vs.contains(Vec3<f32>(1.0, 2.0, 3.0))}')
    println(f'{vs.length()}')
    println(f'{vs[0].x} {vs[0].y} {vs[0].z} | {vs[1].x} {vs[1].y} {vs[1].z}')
";
    assert_runs_with_output(source, "true\ntrue\n2\n1.0 2.0 3.0 | 4.0 5.0 6.0");
}

#[test]
fn a_search_over_a_list_that_is_not_bound_to_a_name_answers_and_releases_it() {
    // The receiver is the result of a call rather than a named place, and one
    // of the searches has its answer discarded, so neither the list nor the
    // element searched for is left to a scope that would release it.
    let source = "
use system.gpu.vector
use system.collections.list

fn made() List<Vec3<f32>>
    return List([Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)])

fn main()
    println(f'{made().contains(Vec3<f32>(4.0, 5.0, 6.0))}')
    var vs = made()
    vs.remove(Vec3<f32>(1.0, 2.0, 3.0))
    println(f'{vs.length()} {vs[0].x} {vs[0].y} {vs[0].z}')
    if vs.contains(Vec3<f32>(4.0, 5.0, 6.0))
        println('still there')
";
    assert_runs_with_output(source, "true\n1 4.0 5.0 6.0\nstill there");
}

/// A vector is copied into the list as its components, so pushing or
/// inserting one hands the list no reference: every vector the program built
/// is released by the program, and the heap guard sees nothing left over.
#[test]
fn vectors_pushed_and_inserted_into_a_list_are_all_released() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List<Vec3<f32>>()
    let kept = Vec3<f32>(1.0, 2.0, 3.0)
    vs.push(kept)
    vs.push(Vec3<f32>(7.0, 8.0, 9.0))
    vs.insert(1, Vec3<f32>(4.0, 5.0, 6.0))
    println(f'{vs[1].y} {kept.z} {vs.length()}')
";
    assert_heap_guard_output(source, "5.0 3.0 3");
}

#[test]
fn an_array_of_vectors_answers_first_last_contains_and_index_of() {
    let source = "
use system.gpu.vector

fn main()
    let vs = [Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)]
    let found = vs.contains(Vec3<f32>(1.0, 2.0, 3.0))
    let missing = vs.contains(Vec3<f32>(1.0, 2.0, 9.0))
    let at = vs.index_of(Vec3<f32>(4.0, 5.0, 6.0)) ?? -1
    match vs.first()
        Some(v): println(f'{v.x} {v.y} {v.z}')
        None: println('none')
    match vs.last()
        Some(v): println(f'{v.x} {v.y} {v.z}')
        None: println('none')
    println(f'{found} {missing} {at}')
";
    assert_runs_with_output(source, "1.0 2.0 3.0\n4.0 5.0 6.0\ntrue false 1");
}

#[test]
fn default_methods_over_a_list_of_vectors_read_every_component() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    let vs = List([Vec3<f32>(1.0, 2.0, 3.0), Vec3<f32>(4.0, 5.0, 6.0)])
    let r = vs.reversed()
    let t = vs.take(1)
    let s = vs.skip(1)
    let f = vs.filter(fn(v Vec3<f32>) bool: v.y > 3.0)
    let any_big = vs.any(fn(v Vec3<f32>) bool: v.z == 6.0)
    let all_big = vs.all(fn(v Vec3<f32>) bool: v.z == 6.0)
    println(f'{r[0].x} {r[0].y} {r[0].z} | {t[0].z} {t.length()} | {s[0].x} | {f[0].z} {f.length()}')
    println(f'{any_big} {all_big}')
";
    assert_runs_with_output(source, "4.0 5.0 6.0 | 3.0 1 | 4.0 | 6.0 1\ntrue false");
}

#[test]
fn arrays_of_vec2_and_vec4_answer_first_and_contains() {
    let source = "
use system.gpu.vector

fn main()
    let twos = [Vec2<i32>(7, 8), Vec2<i32>(9, 10)]
    let fours = [Vec4<f64>(1.0, 2.0, 3.0, 4.0), Vec4<f64>(5.0, 6.0, 7.0, 8.0)]
    match twos.last()
        Some(v): println(f'{v.x} {v.y}')
        None: println('none')
    match fours.first()
        Some(v): println(f'{v.z} {v.w}')
        None: println('none')
    println(f'{twos.contains(Vec2<i32>(9, 10))} {fours.contains(Vec4<f64>(5.0, 6.0, 7.0, 9.0))}')
";
    assert_runs_with_output(source, "9 10\n3.0 4.0\ntrue false");
}
