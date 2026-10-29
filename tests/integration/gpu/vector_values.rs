// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A vector written with its scalar component (`Vec3<f32>`) is held as its
//! bytes, like a struct of scalars: in a frame, inline in the struct, class,
//! tuple, optional or collection holding it, and copied whole wherever it is
//! bound or passed. No vector operation allocates, so a loop doing one a
//! thousand times allocates exactly what the same loop does ten times.
//!
//! Every program here runs under the heap guard, so a vector read through a
//! pointer it never was, or released as an allocation it never was, stops it.

use super::utils::*;

/// `template` with `COUNT` replaced by `count`.
fn at_count(template: &str, count: usize) -> String {
    template.replace("COUNT", &count.to_string())
}

/// Runs `template` at ten and at a thousand iterations under the heap guard,
/// expecting `expected` from both, and asserts both allocate equally: no
/// allocation is made per iteration. Each program prints the same text at
/// both counts, so formatting its output allocates alike.
fn assert_iterations_allocate_nothing(template: &str, expected: &str) {
    let small = at_count(template, 10);
    let large = at_count(template, 1000);
    assert_heap_guard_output(&small, expected);
    assert_heap_guard_output(&large, expected);
    assert_eq!(
        get_allocation_count_value(&small),
        get_allocation_count_value(&large),
        "a vector operation inside the loop allocated"
    );
}

#[test]
fn vector_broadcast_in_a_loop_allocates_nothing() {
    assert_iterations_allocate_nothing(
        "
use system.gpu.vector

fn main()
    var acc = Vec3<f32>(1.0, 2.0, 4.0)
    for i in 0..COUNT
        acc = acc * 1.0
    println(f\"{acc.x} {acc.y} {acc.z}\")
",
        "1.0 2.0 4.0",
    );
}

#[test]
fn vector_constructed_in_a_loop_allocates_nothing() {
    assert_iterations_allocate_nothing(
        "
use system.gpu.vector

fn main()
    var acc = Vec3<f32>(0.0, 0.0, 0.0)
    for i in 0..COUNT
        let d = Vec3<f32>(1.0, 2.0, 3.0)
        acc = Vec3<f32>(acc.x + d.x, acc.y + d.y, acc.z + d.z)
    println(f\"{acc.x / COUNT.0} {acc.y / COUNT.0} {acc.z / COUNT.0}\")
",
        "1.0 2.0 3.0",
    );
}

#[test]
fn vector_fields_arguments_and_results_allocate_nothing() {
    assert_iterations_allocate_nothing(
        "
use system.gpu.vector

struct Body
    pos Vec3<f32>
    vel Vec3<f32>

class Holder
    var pos Vec3<f32>
    fn init(v Vec3<f32>)
        self.pos = v

fn step(p Vec3<f32>, v Vec3<f32>) Vec3<f32>
    return Vec3<f32>(p.x + v.x, p.y + v.y, p.z + v.z)

fn main()
    var b = Body(pos: Vec3<f32>(0.0, 0.0, 0.0), vel: Vec3<f32>(1.0, 1.0, 1.0))
    var h = Holder(Vec3<f32>(1.0, 2.0, 3.0))
    for i in 0..COUNT
        b.pos = step(b.pos, b.vel)
        h.pos = step(h.pos, b.vel)
    println(f\"{b.pos.x / COUNT.0} {h.pos.y - b.pos.x} {h.pos.z - b.pos.x}\")
",
        "1.0 2.0 3.0",
    );
}

#[test]
fn vector_in_an_optional_and_a_tuple_allocates_nothing() {
    assert_iterations_allocate_nothing(
        "
use system.gpu.vector

fn halve(v Vec2<f64>) Vec2<f64>?
    if v.x > 1000000.0
        return None
    return Some(Vec2<f64>(v.x + 1.0, v.y * 0.5))

fn main()
    var pair = (Vec2<f64>(0.0, 8.0), Vec2<f64>(1.0, 1.0))
    for i in 0..COUNT
        let next = halve(pair.0)
        if let Some(v) = next
            pair = (v, pair.1 * 1.0)
    println(f\"{pair.0.x / COUNT.0} {pair.0.y > 0.0} {pair.1.x}\")
",
        "1.0 true 1.0",
    );
}

#[test]
fn generic_function_over_a_vector_allocates_nothing() {
    assert_iterations_allocate_nothing(
        "
use system.gpu.vector

fn first<T>(v Vec2<T>) T
    return v.x

fn main()
    var s = 0
    for i in 0..COUNT
        s = s + first(Vec2<int>(i, 2))
    println(f\"{s * 2 == COUNT * (COUNT - 1)}\")
",
        "true",
    );
}

const LIST_OF_VECTORS: &str = "
use system.gpu.vector
use system.collections.list

fn main()
    var xs = List<Vec3<f32>>()
    for i in 0..1000
        xs.push(Vec3<f32>(1.0, 2.0, 3.0))
    var s = 0.0
READS
    if CHECK
        println(f\"{xs.length()}\")
";

/// A list of vectors read and rewritten element by element allocates only
/// what pushing its elements does: the element bound and the element
/// rebuilt are each held as their bytes.
#[test]
fn vector_list_elements_are_read_and_rewritten_without_allocating() {
    let push_only = LIST_OF_VECTORS
        .replace("READS", "")
        .replace("CHECK", "s == 0.0 and xs[3].x == 1.0 and xs[3].z == 3.0");
    let read_and_rewrite = LIST_OF_VECTORS
        .replace(
            "READS",
            "    for i in 0..1000
        let v = xs[i]
        s = s + v.y
    for i in 0..1000
        xs[i] = xs[i] * 2.0",
        )
        .replace("CHECK", "s == 2000.0 and xs[3].x == 2.0 and xs[3].z == 6.0");
    assert_heap_guard_output(&push_only, "1000");
    assert_heap_guard_output(&read_and_rewrite, "1000");
    assert_eq!(
        get_allocation_count_value(&push_only),
        get_allocation_count_value(&read_and_rewrite),
        "reading or rewriting a vector element allocated"
    );
}

/// An `f32` component written with a float literal is narrowed to the
/// component's width, wherever the vector is held.
#[test]
fn f32_component_writes_take_float_literals_everywhere_a_vector_is_held() {
    assert_heap_guard_output(
        "
use system.gpu.vector
use system.collections.list

struct Body
    pos Vec3<f32>

class Holder
    var pos Vec3<f32>
    fn init(v Vec3<f32>)
        self.pos = v

fn main()
    var v = Vec3<f32>(1.0, 2.0, 3.0)
    v.x = 5.5
    v.y = v.x + 0.25
    var b = Body(pos: Vec3<f32>(0.0, 0.0, 0.0))
    b.pos.z = 2.5
    var h = Holder(Vec3<f32>(0.0, 0.0, 0.0))
    h.pos.y = 7.5
    var xs = List<Vec3<f32>>()
    xs.push(Vec3<f32>(0.0, 0.0, 0.0))
    xs[0].y = 1.5
    println(f\"{v.x} {v.y} {v.z} {b.pos.z} {h.pos.y} {xs[0].y} {xs[0].z}\")
",
        "5.5 5.75 3.0 2.5 7.5 1.5 0.0",
    );
}

/// A vector equal to another component by component is equal, whatever its
/// bytes: `-0.0` equals `0.0`, so a lookup by value compares components.
#[test]
fn vector_equality_compares_components_not_bytes() {
    assert_heap_guard_output(
        "
use system.gpu.vector
use system.collections.list

fn main()
    var xs = List<Vec2<f32>>()
    xs.push(Vec2<f32>(0.0, 1.0))
    let neg = Vec2<f32>(-0.0, 1.0)
    println(f\"{xs.contains(neg)} {xs.contains(Vec2<f32>(0.0, 2.0))}\")
",
        "true false",
    );
}

/// A buffer the host spaces differently from the kernel's `array<E>` is
/// refused when it is built: a `Vec3<int>` is thirty-two bytes on the host and
/// travels as a `vec3<i32>` sixteen bytes apart, and the upload copies the
/// host's bytes as they are.
#[test]
fn vector_buffer_spaced_differently_on_host_and_device_is_refused() {
    assert_build_error(
        "
use system.gpu
use system.gpu.vector
use system.collections.array

fn main()
    gpu let src = [Vec3<int>(1, 2, 3), Vec3<int>(4, 5, 6)]
    gpu var dst = [Vec3<int>(0, 0, 0), Vec3<int>(0, 0, 0)]
    gpu forall i in 0..2
        dst[i] = src[i]
    let host = dst
    println(f\"{host[1].x}\")
",
        "32 bytes apart on the host but 16",
    );
}
