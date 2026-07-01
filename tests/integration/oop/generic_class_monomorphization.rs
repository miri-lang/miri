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

// A managed `T` (String) monomorphizes to a per-instantiation drop thunk
// (`__drop_Box__String`) that DecRefs the field, so the boxed string reads back
// and the box frees cleanly. `assert_runs_with_output` fails on a leak (the
// runtime prints `MIRI_LEAK_CHECK: leaked` and exits non-zero), so this also
// guards against the field not being freed.
#[test]
fn generic_class_string_field_returns_value() {
    assert_runs_with_output(
        "
class Box<T>
    var value T

    public fn get() T: self.value

let b = Box<String>(value: \"hi\")
println(b.get())
",
        "hi",
    );
}

// Constructing then dropping a `Box<String>` without ever reading the field must
// still free the boxed string — isolates the per-instantiation drop thunk from
// any method-return reference counting.
#[test]
fn generic_class_string_field_frees_without_leak() {
    assert_runs_with_output(
        "
class Box<T>
    var value T

    public fn get() T: self.value

let b = Box<String>(value: \"hi\")
println(\"made a box\")
",
        "made a box",
    );
}

// A scalar and a managed instantiation of the same generic class coexist: the
// `Box<int>` field is skipped by its no-op thunk while the `Box<String>` field is
// DecRef'd by its own thunk. A per-generic-class (rather than per-instantiation)
// skip would leak the string here.
#[test]
fn generic_class_int_and_string_instantiations_coexist() {
    assert_runs_with_output(
        "
class Box<T>
    var value T

    public fn get() T: self.value

let a = Box<int>(value: 7)
let b = Box<String>(value: \"hi\")
println(f\"{a.get()}\")
println(b.get())
",
        "7\nhi",
    );
}

// A trait default method returning `T` monomorphizes per instantiation: the
// class inherits the default (does not override it), and a `Box<float>` receiver
// dispatches to a body whose return/parameter types are the concrete `float`, so
// the value round-trips at full precision instead of through an integer slot.
#[test]
fn generic_class_trait_default_returning_type_param_monomorphizes() {
    assert_runs_with_output(
        "
trait Gettable<T>
    fn echo(x T) T
        return x

class Box<T> implements Gettable<T>
    var value T

let b = Box<float>(value: 3.5)
println(f\"{b.echo(1.5)}\")
",
        "1.5",
    );
}

// The int instantiation of the same trait default keeps its pointer-width body,
// proving the mangled dispatch selects the right monomorphization per receiver.
#[test]
fn generic_class_trait_default_returning_type_param_int_instantiation() {
    assert_runs_with_output(
        "
trait Gettable<T>
    fn echo(x T) T
        return x

class Box<T> implements Gettable<T>
    var value T

let b = Box<int>(value: 7)
println(f\"{b.echo(9) + b.echo(3)}\")
",
        "12",
    );
}

// A managed `T` inheriting a trait-default returning `T` routes through the bare
// `Box_echo` body (managed args are already pointer-shaped, so no scalar-width
// monomorphization is needed) and frees cleanly — `assert_runs_with_output`
// fails on a leak, so this guards the managed trait-default path too.
#[test]
fn generic_class_trait_default_returning_managed_type_param() {
    assert_runs_with_output(
        "
trait Gettable<T>
    fn echo(x T) T
        return x

class Box<T> implements Gettable<T>
    var value T

let b = Box<String>(value: \"hi\")
println(b.echo(\"world\"))
",
        "world",
    );
}

// When a generic class overrides the trait default with its own method, the
// own-method monomorphization path supplies the mangled body and the trait
// default is skipped — no double emission, and the override result is used.
#[test]
fn generic_class_overriding_trait_default_uses_own_method() {
    assert_runs_with_output(
        "
trait Gettable<T>
    fn echo(x T) T
        return x

class Box<T> implements Gettable<T>
    var value T

    fn echo(x T) T
        return x

let b = Box<float>(value: 3.5)
println(f\"{b.echo(2.5)}\")
",
        "2.5",
    );
}

// Regression guard: a `List` of managed elements must still free its elements
// even though generic-class drop thunks now exist. `List` routes through the
// runtime `miri_rt_list_free` decref path, never the generic-class thunk, so a
// coexisting `Box<String>` must not perturb collection element cleanup.
#[test]
fn list_of_strings_still_frees_alongside_generic_class() {
    assert_runs_with_output(
        "
use system.collections.list

class Box<T>
    var value T

    public fn get() T: self.value

var words = List([\"hello\", \"world\", \"foo\"])
words.remove_at(0)
let b = Box<String>(value: \"boxed\")
println(f\"{words.length()}\")
",
        "2",
    );
}

// A generic container reads an element out of a `List<T>` field. Inside the
// monomorphized method the intrinsic element read is typed `T`; without the
// substitution it falls back to the pointer-width `Int`, so a `T = f32` element
// is loaded at the wrong width and the value is garbage. The read must resolve
// `T` to the instantiation's concrete type so the element round-trips.
#[test]
fn generic_container_element_read_substitutes_type_param() {
    assert_runs_with_output(
        "
use system.collections.list

class Container<T>
    var items List<T>

    public fn first() T: self.items.element_at(0)

let c = Container<f32>(items: List([1.5, 2.5]))
println(f\"{c.first()}\")
",
        "1.5",
    );
}

// The pointer-width instantiation and the `get` alias route through the same
// substituted element read: `T = int` loads at pointer width and `get(0)`
// resolves to the concrete type just as `element_at(0)` does.
#[test]
fn generic_container_int_element_read_and_get_alias() {
    assert_runs_with_output(
        "
use system.collections.list

class Container<T>
    var items List<T>

    public fn first() T: self.items.element_at(0)
    public fn viaget() T: self.items.get(0)

let c = Container<int>(items: List([10, 20]))
println(f\"{c.first() + c.viaget()}\")
",
        "20",
    );
}

// A managed `T` container reads the boxed element through the same substitution
// (`List<String>`), returning the value at pointer width and freeing cleanly —
// `assert_runs_with_output` fails on a leak, so this guards the managed path.
#[test]
fn generic_container_managed_element_read_substitutes_type_param() {
    assert_runs_with_output(
        "
use system.collections.list

class Container<T>
    var items List<T>

    public fn first() T: self.items.element_at(0)

let c = Container<String>(items: List([\"hi\", \"yo\"]))
println(c.first())
",
        "hi",
    );
}
