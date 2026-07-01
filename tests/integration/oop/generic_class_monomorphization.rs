// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// Generic-class method monomorphization for a scalar type parameter.
//
// A `class Box<T>` with a `value T` field and a `fn get() T` method compiles a
// per-instantiation method body (`Box_get__int`) whose return type is the
// concrete instantiation type, not the opaque generic `T`. The call site emits
// the byte-identical mangled symbol and types the result as the concrete type,
// so the value round-trips end-to-end.
//
// Every non-managed scalar `T` monomorphizes: the `value` field lays out at the
// instantiation's concrete width (a pointer-width `int`, a 64-bit `float`, a
// 32-bit `f32`), so the load/store is byte-exact. Managed `T` freeing needs a
// per-instantiation drop thunk and stays blocked here until its own step lands.

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

// A non-pointer-width scalar `T` (float) monomorphizes: the `value` field lays
// out as an f64, so `get()` reads it back at full precision.
#[test]
fn generic_class_float_field_returns_value() {
    assert_runs_with_output(
        "
class Box<T>
    var value T

    public fn get() T: self.value

let b = Box<float>(value: 3.5)
println(f\"{b.get()}\")
",
        "3.5",
    );
}

// The narrower `f32` scalar also monomorphizes at its own 4-byte field width.
#[test]
fn generic_class_f32_field_returns_value() {
    assert_runs_with_output(
        "
class Box<T>
    var value T

    public fn get() T: self.value

let b = Box<f32>(value: 2.5)
println(f\"{b.get()}\")
",
        "2.5",
    );
}

// A monomorphized float field flows into float arithmetic, proving the load
// produces a float register value rather than reinterpreted integer bits.
#[test]
fn generic_class_float_field_participates_in_arithmetic() {
    assert_runs_with_output(
        "
class Box<T>
    var value T

    public fn get() T: self.value

let b = Box<float>(value: 1.5)
println(f\"{b.get() + b.get()}\")
",
        "3",
    );
}

// A user `init` method must dispatch to the per-instantiation body so the
// constructor argument crosses the ABI at the concrete scalar width. A bare
// `Box_init` call would pass the f64 through an integer slot and corrupt it.
#[test]
fn generic_class_float_init_method_stores_at_scalar_width() {
    assert_runs_with_output(
        "
class Box<T>
    var value T

    public fn init(v T)
        self.value = v

    public fn get() T: self.value

let b = Box<float>(3.5)
println(f\"{b.get()}\")
",
        "3.5",
    );
}

// Two scalar instantiations of the same class (`Box<int>` and `Box<float>`)
// coexist: each field lays out at its own width and both share one bare-name
// drop thunk that safely skips the non-managed scalar field.
#[test]
fn generic_class_mixed_scalar_instantiations_coexist() {
    assert_runs_with_output(
        "
class Box<T>
    var value T

    public fn get() T: self.value

let a = Box<int>(value: 7)
let b = Box<float>(value: 3.5)
println(f\"{a.get()}\")
println(f\"{b.get()}\")
",
        "7\n3.5",
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
