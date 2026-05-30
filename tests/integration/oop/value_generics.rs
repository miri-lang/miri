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
use system.io
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
    // Literal `[1.5, 2.5, 3.5]` is `Array<f32, 3>`. Declaring the field as
    // `Array<float, 3>` (= F64 storage) would put a 4-byte-stride buffer
    // beneath an 8-byte-stride reader. The field-type check refuses it.
    assert_compiler_error(
        "
use system.collections.array

class Wrap<T, Size>
    var data Array<T, Size>

let w = Wrap<float, 3>(data: [1.5, 2.5, 3.5])
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
