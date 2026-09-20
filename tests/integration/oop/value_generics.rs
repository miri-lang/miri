// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// Value-generic user classes: a `class C<T, Size>` with an `Array<T, Size>`
// field threads both the type parameter and the value-generic `Size` slot
// into constructor field-type checking (`validate_class_field_args`). The
// element width and the fixed size must both match the literal argument.

use super::utils::*;

#[test]
fn value_generic_class_accepts_matching_field_literal() {
    assert_runs_with_output(
        "
use system.collections.array

class Wrap<T, Size>
    var data Array<T, Size>

    public fn length() int: self.data.length()

let w = Wrap<int, 3>(data: [10, 20, 30])
println(f\"{w.length()}\")
",
        "3",
    );
}

#[test]
fn value_generic_class_rejects_layout_incompatible_element_width() {
    // `d` is a value with `float` (F64) storage, so it does not narrow the way
    // a literal would. Binding it to an `Array<f32, 3>` field would put an
    // 8-byte-stride buffer beneath a 4-byte-stride reader. The field-type check
    // refuses it.
    assert_compiler_error(
        "
use system.collections.array

class Wrap<T, Size>
    var data Array<T, Size>

let d Array<float,3> = [1.5, 2.5, 3.5]
let w = Wrap<f32, 3>(data: d)
",
        "Type mismatch for field 'data'",
    );
}

#[test]
fn value_generic_class_rejects_size_mismatch_with_literal() {
    // `[1, 2, 3]` is `Array<int, 3>`. `Wrap<int, 4>` declares the field as
    // `Array<int, 4>`, so the value-generic `Size` slot carries the
    // constraint into constructor type-checking.
    assert_compiler_error(
        "
use system.collections.array

class Wrap<T, Size>
    var data Array<T, Size>

let w = Wrap<int, 4>(data: [1, 2, 3])
",
        "Type mismatch for field 'data'",
    );
}

/// A class that declares a value parameter alongside a type parameter still
/// has to release a managed field. The value argument is a literal rather than
/// a type, and naming the instantiation's drop thunk means accounting for that
/// — otherwise the shared thunk answers instead, and that one skips a field
/// still written at a parameter.
#[test]
fn a_value_generic_class_releases_a_bare_parameter_field() {
    assert_heap_guard_output(
        r#"
class Buf<T, Size>
    var tag T

fn main()
    var b = Buf<String, 2>(tag: "x" + "y")
    println(f"{b.tag}")
"#,
        "xy",
    );
}

#[test]
fn a_value_generic_class_releases_a_managed_field_beside_a_sized_array() {
    assert_heap_guard_output(
        r#"
class Buf<T, Size>
    var items Array<T, Size>
    var tag T

fn main()
    var b = Buf<String, 2>(items: ["a" + "b", "c" + "d"], tag: "x" + "y")
    println(f"{b.tag}")
"#,
        "xy",
    );
}

#[test]
fn a_value_generic_class_releases_its_fields_in_either_order() {
    assert_heap_guard_output(
        r#"
class Buf<T, Size>
    var tag T
    var items Array<T, Size>

fn main()
    var b = Buf<String, 2>(tag: "x" + "y", items: ["a" + "b", "c" + "d"])
    println(f"{b.tag}")
"#,
        "xy",
    );
}

/// The value parameter is declared first, so anything that pairs arguments to
/// parameters by position has to stay right when the one it cannot resolve
/// comes before the one it can.
#[test]
fn a_value_parameter_declared_first_still_leaves_the_type_parameter_bound() {
    assert_heap_guard_output(
        r#"
class Buf<Size, T>
    var tag T

fn main()
    var b = Buf<2, String>(tag: "x" + "y")
    println(f"{b.tag}")
"#,
        "xy",
    );
}

#[test]
fn a_value_generic_class_at_a_scalar_argument_stays_balanced() {
    assert_heap_guard_output(
        r#"
class Buf<T, Size>
    var items Array<T, Size>
    var tag T

fn main()
    var b = Buf<int, 2>(items: [1, 2], tag: 3)
    println(f"{b.tag}")
"#,
        "3",
    );
}

#[test]
fn a_three_parameter_mix_releases_the_managed_field_after_the_value() {
    assert_heap_guard_output(
        r#"
class Buf<T, Size, U>
    var items Array<T, Size>
    var extra U

fn main()
    var b = Buf<int, 2, String>(items: [1, 2], extra: "x" + "y")
    println(f"{b.extra}")
"#,
        "xy",
    );
}

#[test]
fn two_instantiations_of_one_value_generic_class_each_release_their_own_field() {
    assert_heap_guard_output(
        r#"
class Buf<T, Size>
    var tag T

fn main()
    var a = Buf<String, 2>(tag: "x" + "y")
    var b = Buf<String, 3>(tag: "z" + "w")
    println(f"{a.tag} {b.tag}")
"#,
        "xy zw",
    );
}
