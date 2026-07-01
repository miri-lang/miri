// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// Generic-class method monomorphization for a pointer-width integer parameter.
//
// A `class Box<T>` with a `value T` field and a `fn get() T` method compiles a
// per-instantiation method body (`Box_get__int`) whose return type is the
// concrete instantiation type, not the opaque generic `T`. The call site emits
// the byte-identical mangled symbol and types the result as the concrete type,
// so the value round-trips end-to-end. `int` is the proven-safe slice: it shares
// the pointer register width, so the generic field's load/store is exact.
//
// Non-pointer-width scalar `T` (e.g. `float`) and managed `T` freeing are gated
// separately and stay blocked here until their own steps land.

use super::utils::*;

#[test]
fn generic_class_int_method_returns_field_value() {
    assert_runs_with_output(
        "
class Box<T>
    var value T

    public fn get() T: self.value

let b = Box<int>(value: 3)
println(f\"{b.get()}\")
",
        "3",
    );
}

#[test]
fn generic_class_int_method_participates_in_arithmetic() {
    // The monomorphized result is typed `int`, so it flows into integer
    // arithmetic instead of being treated as an opaque managed pointer.
    assert_runs_with_output(
        "
class Box<T>
    var value T

    public fn get() T: self.value

let b = Box<int>(value: 40)
let doubled = b.get() + b.get()
println(f\"{doubled}\")
",
        "80",
    );
}

#[test]
fn generic_class_int_method_with_parameter_substitutes() {
    // A method parameter typed `T` is substituted to the concrete `int` in the
    // monomorphized body, so it accepts an integer argument directly.
    assert_runs_with_output(
        "
class Box<T>
    var value T

    public fn add(other T) T: self.value + other

let b = Box<int>(value: 5)
println(f\"{b.add(7)}\")
",
        "12",
    );
}

#[test]
fn generic_class_int_two_instances_share_one_monomorphization() {
    // Two `Box<int>` instances deduplicate onto the same `Box_get__int` body
    // and both drop cleanly (the field is a non-managed pointer-width int, so
    // the bare-name drop thunk is a safe no-op).
    assert_runs_with_output(
        "
class Box<T>
    var value T

    public fn get() T: self.value

let a = Box<int>(value: 11)
let b = Box<int>(value: 31)
println(f\"{a.get() + b.get()}\")
",
        "42",
    );
}

// The F8c slice is pointer-width integers only. A non-pointer-width scalar `T`
// (float) needs a per-instantiation field width, so it is rejected at codegen
// rather than silently reading the field through the wrong register width.
#[test]
fn generic_class_float_field_is_rejected_until_scalar_width_lands() {
    assert_build_error(
        "
class Box<T>
    var value T

    public fn get() T: self.value

let b = Box<float>(value: 3.5)
println(f\"{b.get()}\")
",
        "bare-generic field",
    );
}

// A managed `T` (String) needs a per-instantiation drop thunk to free the field;
// the shared bare-name thunk cannot encode that, so it stays fail-closed here.
#[test]
fn generic_class_managed_field_is_rejected_until_drop_thunks_land() {
    assert_build_error(
        "
class Box<T>
    var value T

    public fn get() T: self.value

let b = Box<String>(value: \"hi\")
println(b.get())
",
        "bare-generic field",
    );
}

// Two conflicting instantiations of the same generic class cannot share one
// bare-name drop thunk (one skips, one would DecRef), so the mix is rejected.
#[test]
fn generic_class_conflicting_instantiations_are_rejected() {
    assert_build_error(
        "
class Box<T>
    var value T

    public fn get() T: self.value

let a = Box<int>(value: 7)
let b = Box<String>(value: \"x\")
println(f\"{a.get()}\")
println(b.get())
",
        "bare-generic field",
    );
}
