// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A vector's component type decides how the vector is laid out: every accepted
//! component has an inline byte width, so a collection knows the stride to place
//! its elements at and reference counting knows the element holds bytes rather
//! than a pointer. A component with no such width has no layout to agree on, and
//! is refused where it is written rather than read back as zeros or garbage.

use super::utils::*;

/// Every component listed as supported still constructs and reads back, so the
/// rejection below narrows nothing that worked.
#[test]
fn a_vector_at_every_supported_component_reads_back_its_values() {
    let source = "
use system.gpu.vector

fn main()
    let a = Vec2<f32>(1.5, 2.5)
    let b = Vec2<i32>(1, 2)
    let c = Vec2<u32>(3, 4)
    let d = Vec2<f64>(5.5, 6.5)
    let e = Vec2<i64>(7, 8)
    let f = Vec2<u64>(9, 10)
    let g = Vec2<int>(11, 12)
    let h = Vec2<float>(13.5, 14.5)
    println(f'{a.x} {b.y} {c.x} {d.y} {e.x} {f.y} {g.x} {h.y}')
";
    assert_runs_with_output(source, "1.5 2 3 6.5 7 10 11 14.5");
}

/// A component nobody wrote is inferred from the arguments, and the inferred
/// widths are supported ones — so the refusal never fires on a vector written
/// without type arguments.
#[test]
fn a_vector_with_no_written_component_infers_a_supported_one() {
    let source = "
use system.gpu.vector

fn main()
    let whole = Vec2(1, 2)
    let fractional = Vec3(1.5, 2.5, 3.5)
    println(f'{whole.x} {whole.y} {fractional.z}')
";
    assert_runs_with_output(source, "1 2 3.5");
}

/// Narrow integer components stay refused, and this is the decision rather
/// than a gap waiting to be filled. WGSL has no narrow integer scalar — `i8`
/// and `i16` both emit `i32` — so a narrow component cannot exist on the device
/// at the width it was written at. Storing four bytes for it on the host to
/// make the two agree would leave `Vec2<i16>` a `Vec2<i32>` under another name,
/// and letting the host keep two bytes is what made a vector in a collection
/// read back zeros.
#[test]
fn a_vector_component_narrower_than_four_bytes_is_refused() {
    for component in ["i8", "i16", "u8", "u16"] {
        let source = format!(
            "
use system.gpu.vector

fn main()
    let v = Vec2<{component}>(1, 2)
    println(f'{{v.x}}')
"
        );
        assert_compiler_error(
            &source,
            &format!("Vector component type '{component}' is not supported"),
        );
    }
}

#[test]
fn a_half_precision_vector_component_is_refused() {
    let source = "
use system.gpu.vector

fn main()
    let v = Vec2<f16>(1.0, 2.0)
    println(f'{v.x}')
";
    assert_compiler_error(source, "Vector component type 'f16' is not supported");
}

#[test]
fn a_boolean_vector_component_is_refused() {
    let source = "
use system.gpu.vector

fn main()
    let v = Vec2<bool>(true, false)
    println(f'{v.x}')
";
    assert_compiler_error(source, "Vector component type 'bool' is not supported");
}

#[test]
fn a_128_bit_vector_component_is_refused() {
    let source = "
use system.gpu.vector

fn main()
    let v = Vec2<i128>(1, 2)
    println(f'{v.x}')
";
    assert_compiler_error(source, "Vector component type 'i128' is not supported");
}

#[test]
fn a_string_vector_component_is_refused() {
    let source = "
use system.gpu.vector

fn main()
    let v = Vec2<String>('a', 'b')
    println(f'{v.x}')
";
    assert_compiler_error(source, "Vector component type 'String' is not supported");
}

/// The reported error names the components that do work, so the reader can act
/// on it without reading the compiler.
#[test]
fn the_refusal_names_the_components_that_are_supported() {
    let source = "
use system.gpu.vector

fn main()
    let v = Vec3<i16>(1, 2, 3)
    println(f'{v.x}')
";
    assert_compiler_error(source, "f32, i32, u32, f64, i64, u64, int, float");
}

/// The collection case this refusal exists for: an unsupported component has no
/// inline layout, so the element was stored as a pointer, left unretained, and
/// read back as zeros.
#[test]
fn an_unsupported_component_is_refused_inside_a_collection_literal() {
    let source = "
use system.gpu.vector

fn main()
    let a = [Vec2<i16>(1, 2), Vec2<i16>(3, 4)]
    println(f'{a[0].x} {a[1].y}')
";
    assert_compiler_error(source, "Vector component type 'i16' is not supported");
}

/// The component is unsupported wherever it is written, not only at a
/// constructor: a declared element type reaches layout the same way.
#[test]
fn an_unsupported_component_is_refused_in_a_declared_collection_type() {
    let source = "
use system.gpu.vector
use system.collections.list

fn main()
    var vs = List<Vec2<u8>>()
    println(f'{vs.length()}')
";
    assert_compiler_error(source, "Vector component type 'u8' is not supported");
}

#[test]
fn an_unsupported_component_is_refused_in_a_parameter_type() {
    let source = "
use system.gpu.vector

fn take(v Vec2<i16>) i16
    return v.x

fn main()
    println('ok')
";
    assert_compiler_error(source, "Vector component type 'i16' is not supported");
}

#[test]
fn an_unsupported_component_is_refused_in_a_class_field_type() {
    let source = "
use system.gpu.vector

class Holder
    v Vec2<bool>

fn main()
    println('ok')
";
    assert_compiler_error(source, "Vector component type 'bool' is not supported");
}

/// A generic signature is where a bad component could otherwise hide: the
/// parameter passes the declaration unjudged, but the value handed to it has to
/// be built, and the constructor is what refuses it.
#[test]
fn an_unsupported_component_is_refused_where_a_generic_signature_is_instantiated() {
    let source = "
use system.gpu.vector

fn first<T>(v Vec2<T>) T
    return v.x

fn main()
    println(f'{first(Vec2<i16>(1, 2))}')
";
    assert_compiler_error(source, "Vector component type 'i16' is not supported");
}

#[test]
fn an_unsupported_component_is_refused_in_a_generic_class_field() {
    let source = "
use system.gpu.vector

class Holder<T>
    v Vec2<T>

fn main()
    let h = Holder<i16>(Vec2<i16>(1, 2))
    println(f'{h.v.x}')
";
    assert_compiler_error(source, "Vector component type 'i16' is not supported");
}

/// A struct component is the case a generic parameter is easily mistaken for:
/// both are named types the checker sees bare. This one has no inline layout —
/// left admitted, a collection of these SIGSEGVs.
#[test]
fn a_struct_vector_component_is_refused() {
    let source = "
use system.gpu.vector

struct Point
    a int

fn main()
    let vs = [Vec2<Point>(Point(1), Point(2)), Vec2<Point>(Point(3), Point(4))]
    println(f'{vs[0].x.a} {vs[1].y.a}')
";
    assert_compiler_error(source, "Vector component type 'Point' is not supported");
}

#[test]
fn a_vector_component_that_is_itself_a_vector_is_refused() {
    let source = "
use system.gpu.vector

fn main()
    let v = Vec2<Vec2<f32>>(Vec2<f32>(1.0, 2.0), Vec2<f32>(3.0, 4.0))
    println(f'{v.x.x}')
";
    assert_compiler_error(source, "Vector component type 'Vec2<f32>' is not supported");
}

#[test]
fn an_optional_vector_component_is_refused() {
    let source = "
use system.gpu.vector

fn main()
    let v = Vec2<int?>(1, 2)
    println(f'{v.x}')
";
    assert_compiler_error(source, "Vector component type 'int?' is not supported");
}

/// A component reaching the vector through an alias is the same component: the
/// alias is followed before the question is asked.
#[test]
fn an_unsupported_component_is_refused_through_a_type_alias() {
    let source = "
use system.gpu.vector

type Small is i16

fn main()
    let v = Vec2<Small>(1, 2)
    println(f'{v.x}')
";
    assert_compiler_error(source, "Vector component type 'i16' is not supported");
}

/// A vector written at a generic parameter is not a component the checker can
/// judge; refusing it here would refuse the stdlib's own declarations.
#[test]
fn a_vector_at_a_generic_parameter_is_not_refused() {
    let source = "
use system.gpu.vector

fn first<T>(v Vec2<T>) T
    return v.x

fn main()
    println(f'{first(Vec2<i32>(4, 5))}')
";
    assert_runs_with_output(source, "4");
}
