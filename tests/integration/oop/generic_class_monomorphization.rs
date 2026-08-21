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

// A two-parameter generic class monomorphizes each field at its own concrete
// width and mangles the method by both type arguments in declaration order
// (`Pair_first__int__float`). Reading each field back proves the K and V slots
// are laid out and dispatched independently.
#[test]
fn generic_pair_two_scalar_params_round_trip() {
    assert_runs_with_output(
        "
class Pair<K, V>
    var key K
    var val V

    public fn first() K: self.key
    public fn second() V: self.val

let p = Pair<int, float>(key: 3, val: 2.5)
println(f\"{p.first()}\")
println(f\"{p.second()}\")
",
        "3\n2.5",
    );
}

// Two distinct instantiations of the same two-parameter class coexist: the
// argument order distinguishes the mangled symbols (`Pair_first__int__float`
// vs `Pair_first__String__int`), so neither monomorphization collides.
#[test]
fn generic_pair_distinct_instantiations_do_not_collide() {
    assert_runs_with_output(
        "
class Pair<K, V>
    var key K
    var val V

    public fn first() K: self.key
    public fn second() V: self.val

let a = Pair<int, float>(key: 1, val: 2.5)
let b = Pair<String, int>(key: \"x\", val: 9)
println(f\"{a.first()}\")
println(f\"{a.second()}\")
println(b.first())
println(f\"{b.second()}\")
",
        "1\n2.5\nx\n9",
    );
}

// A managed field followed by a scalar field: the drop thunk must DecRef the
// `String` at its own offset (field 0) while skipping the scalar `int` (field
// 1). `assert_runs_with_output` fails on a leak, so this guards multi-field
// offset substitution in the per-instantiation drop path.
#[test]
fn generic_pair_managed_then_scalar_frees_at_correct_offset() {
    assert_runs_with_output(
        "
class Pair<K, V>
    var key K
    var val V

    public fn first() K: self.key
    public fn second() V: self.val

let p = Pair<String, int>(key: \"hi\", val: 7)
println(p.first())
println(f\"{p.second()}\")
",
        "hi\n7",
    );
}

// The mirror layout — a scalar field ahead of a managed one: the drop thunk
// skips the scalar `int` (field 0) and DecRefs the `String` at field 1's
// offset. Leak-guarded like its sibling above.
#[test]
fn generic_pair_scalar_then_managed_frees_at_correct_offset() {
    assert_runs_with_output(
        "
class Pair<K, V>
    var key K
    var val V

    public fn first() K: self.key
    public fn second() V: self.val

let p = Pair<int, String>(key: 5, val: \"yo\")
println(f\"{p.first()}\")
println(p.second())
",
        "5\nyo",
    );
}

// Both fields managed: the drop thunk DecRefs each `String` at its own offset.
// A leak (or a double-free at the wrong offset) fails the run.
#[test]
fn generic_pair_two_managed_fields_free() {
    assert_runs_with_output(
        "
class Pair<K, V>
    var key K
    var val V

    public fn first() K: self.key
    public fn second() V: self.val

let p = Pair<String, String>(key: \"a\", val: \"b\")
println(p.first())
println(p.second())
",
        "a\nb",
    );
}

// A trait default method whose parameter and return use the trait's own generic
// name (`U`), implemented by a class that binds it under a different name
// (`class Box<T> implements Gettable<T>`). The `U → T → concrete` remap must run
// so a `Box<float>` receiver dispatches to a body typed at `float`; without it
// the return stays the opaque trait param `U`.
#[test]
fn generic_class_trait_default_remaps_differing_param_name_float() {
    assert_runs_with_output(
        "
trait Gettable<U>
    fn echo(x U) U
        return x

class Box<T> implements Gettable<T>
    var value T

let b = Box<float>(value: 3.5)
println(f\"{b.echo(1.5)}\")
",
        "1.5",
    );
}

// The int instantiation of the same differing-name trait default keeps its
// pointer-width body — proving the remap composes with the concrete type, not
// just the parameter name.
#[test]
fn generic_class_trait_default_remaps_differing_param_name_int() {
    assert_runs_with_output(
        "
trait Gettable<U>
    fn echo(x U) U
        return x

class Box<T> implements Gettable<T>
    var value T

let b = Box<int>(value: 7)
println(f\"{b.echo(9) + b.echo(3)}\")
",
        "12",
    );
}

// A managed `T` under a differing-name trait default routes through the bare
// pointer-width body and frees cleanly — leak-guarded.
#[test]
fn generic_class_trait_default_remaps_differing_param_name_managed() {
    assert_runs_with_output(
        "
trait Gettable<U>
    fn echo(x U) U
        return x

class Box<T> implements Gettable<T>
    var value T

let b = Box<String>(value: \"hi\")
println(b.echo(\"world\"))
",
        "world",
    );
}

// A generic class instantiated with the wrong number of type arguments is
// rejected at the constructor, before any monomorphization runs.
#[test]
fn generic_pair_wrong_type_arg_count_is_rejected() {
    assert_compiler_error(
        "
class Pair<K, V>
    var key K
    var val V

let p = Pair<int>(key: 3, val: 4)
",
        "expects 2 generic arguments, got 1",
    );
}

// A constructor argument whose type does not match the instantiation's concrete
// field type is rejected with a field type-mismatch diagnostic.
#[test]
fn generic_box_constructor_arg_type_mismatch_is_rejected() {
    assert_compiler_error(
        "
class Box<T>
    var value T

let b = Box<int>(value: \"notint\")
",
        "Type mismatch for field 'value'",
    );
}

// A method argument typed `T` is substituted to the concrete instantiation
// type, so passing an incompatible argument is a compile-time type error.
#[test]
fn generic_box_method_arg_type_mismatch_is_rejected() {
    assert_compiler_error(
        "
class Box<T>
    var value T

    public fn add(other T) T: self.value + other

let b = Box<int>(value: 3)
let r = b.add(\"str\")
",
        "expected int, got String",
    );
}

// A generic class implementing Queryable<T> instantiated at float must have the
// trait-default first() method dispatch to a body whose return type is the
// concrete float, not the uninstantiated pointer-width int. This guards against
// double lowering of trait defaults (uninstantiated + per-instantiation) causing
// conflicting symbol declarations.
//
// BLOCKED: This test uses List<float>.push(), which has a pre-existing FFI bug
// (cast_value_with_sign uses numeric conversion instead of bitcast). The type
// substitution fix is correct, but the underlying float list bug prevents the
// test from passing. Proven with non-generic List<float> tests in float_collections.rs.
#[test]
#[ignore = "Blocked by pre-existing List<float> push() FFI bug (documented in tests/integration/list/float_collections.rs)"]
fn generic_queryable_float_trait_default_uses_concrete_width() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.queryable

class Bag<T> implements Queryable<T>
    private var items List<T>

    fn init()
        self.items = List<T>()

    public fn length() int
        return self.items.length()

    public fn element_at(index int) T
        return self.items.element_at(index)

    public fn add(item T)
        self.items.push(item)

fn main()
    var b = Bag<float>()
    b.add(2.5)
    println(f\"{b.first() ?? 2.5}\")
",
        "2.5",
    );
}

// Additional test: u8 instantiation
#[test]
fn generic_queryable_u8_trait_default_uses_concrete_width() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.queryable

class Bag<T> implements Queryable<T>
    private var items List<T>

    fn init()
        self.items = List<T>()

    public fn length() int
        return self.items.length()

    public fn element_at(index int) T
        return self.items.element_at(index)

    public fn add(item T)
        self.items.push(item)

fn main()
    var b = Bag<u8>()
    b.add(255)
    println(f\"{b.first() ?? 0}\")
",
        "255",
    );
}

// The bool instantiation of the same generic queryable must dispatch to a body
// typed at the concrete bool width, not the uninstantiated int width.
#[test]
fn generic_queryable_bool_trait_default_uses_concrete_width() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.queryable

class Bag<T> implements Queryable<T>
    private var items List<T>

    fn init()
        self.items = List<T>()

    public fn length() int
        return self.items.length()

    public fn element_at(index int) T
        return self.items.element_at(index)

    public fn add(item T)
        self.items.push(item)

fn main()
    var b = Bag<bool>()
    b.add(true)
    var c = Bag<bool>()
    c.add(false)
    println(f\"{b.first() ?? false}\")
    println(f\"{c.first() ?? true}\")
",
        "true
false",
    );
}

// A wide unsigned instantiation must round-trip its boundary value: a body typed
// at the uninstantiated pointer width would sign-extend and print a negative
// number instead.
#[test]
fn generic_queryable_u32_trait_default_uses_concrete_width() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.queryable

class Bag<T> implements Queryable<T>
    private var items List<T>

    fn init()
        self.items = List<T>()

    public fn length() int
        return self.items.length()

    public fn element_at(index int) T
        return self.items.element_at(index)

    public fn add(item T)
        self.items.push(item)

fn main()
    var b = Bag<u32>()
    b.add(4294967295)
    println(f\"{b.first() ?? 0}\")
",
        "4294967295",
    );
}

// The i32 instantiation of the same generic queryable must dispatch correctly.
#[test]
fn generic_queryable_i32_trait_default_uses_concrete_width() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.queryable

class Bag<T> implements Queryable<T>
    private var items List<T>

    fn init()
        self.items = List<T>()

    public fn length() int
        return self.items.length()

    public fn element_at(index int) T
        return self.items.element_at(index)

    public fn add(item T)
        self.items.push(item)

fn main()
    var b = Bag<i32>()
    b.add(5)
    println(f\"{b.first() ?? 0}\")
",
        "5",
    );
}

// Test i64 instantiation of generic queryable trait default
#[test]
fn generic_queryable_i64_trait_default_uses_concrete_width() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.queryable

class Bag<T> implements Queryable<T>
    private var items List<T>

    fn init()
        self.items = List<T>()

    public fn length() int
        return self.items.length()

    public fn element_at(index int) T
        return self.items.element_at(index)

    public fn add(item T)
        self.items.push(item)

fn main()
    var b = Bag<i64>()
    b.add(9223372036854775807)
    println(f\"{b.first() ?? 0}\")
",
        "9223372036854775807",
    );
}

// Test f32 instantiation of generic queryable trait default
#[test]
#[ignore = "List<f32> push/element_at fails at codegen (pre-existing float list bug)"]
fn generic_queryable_f32_trait_default_uses_concrete_width() {
    assert_runs_with_output(
        "
use system.collections.list
use system.collections.queryable

class Bag<T> implements Queryable<T>
    private var items List<T>

    fn init()
        self.items = List<T>()

    public fn length() int
        return self.items.length()

    public fn element_at(index int) T
        return self.items.element_at(index)

    public fn add(item T)
        self.items.push(item)

fn main()
    var b = Bag<f32>()
    b.add(3.14)
    println(f\"{b.first() ?? 0.0}\")
",
        "3.14",
    );
}

// Test generic class with List<T> field - direct element_at without trait default
#[test]
fn generic_bag_list_field_direct_element_at_i32() {
    assert_runs_with_output(
        "
use system.collections.list

class Container<T>
    private var items List<T>

    fn init()
        self.items = List<T>()

    public fn add_item(x T)
        self.items.push(x)

    public fn get_item(i int) T
        return self.items.element_at(i)

fn main()
    var c = Container<i32>()
    c.add_item(42)
    println(f\"{c.get_item(0)}\")
",
        "42",
    );
}

// Test generic class with direct T field at float width (no List)
#[test]
fn generic_box_direct_field_float() {
    assert_runs_with_output(
        "
class Box<T>
    private var value T

    fn init(v T)
        self.value = v

    public fn get() T
        return self.value

fn main()
    var b = Box<float>(2.5)
    println(f\"{b.get()}\")
",
        "2.5",
    );
}

// Test generic class with List<T> field at float width
#[test]
#[ignore = "Pre-existing List<float> push bug (FFI coercion uses numeric conversion instead of bitcast)"]
fn generic_bag_list_field_direct_element_at_float() {
    assert_runs_with_output(
        "
use system.collections.list

class Container<T>
    private var items List<T>

    fn init()
        self.items = List<T>()

    public fn add_item(x T)
        self.items.push(x)

    public fn get_item(i int) T
        return self.items.element_at(i)

fn main()
    var c = Container<float>()
    c.add_item(2.5)
    println(f\"{c.get_item(0)}\")
",
        "2.5",
    );
}

// Test two-parameter generic class at different scalar widths
#[test]
fn generic_pair_two_scalar_widths() {
    assert_runs_with_output(
        "
class Pair<K, V>
    private var key K
    private var value V

    fn init(k K, v V)
        self.key = k
        self.value = v

    public fn get_key() K
        return self.key

fn main()
    var p1 = Pair<i32, i64>(42, 9223372036854775807)
    println(f\"{p1.get_key()}\")
    var p2 = Pair<u8, i32>(255, 123)
    println(f\"{p2.get_key()}\")
",
        "42
255",
    );
}

// A class instance is a type argument like any other. Two instantiations of one
// generic class at two different classes must each read back their own field:
// when both mangle to the same symbol, the second instantiation runs the first
// one's body against its own layout.
#[test]
fn generic_class_at_two_distinct_class_arguments_keeps_them_apart() {
    assert_runs_with_output(
        "
class Widget
    public var tag int
    fn init(t int)
        self.tag = t

class Gadget
    public var label String
    fn init(l String)
        self.label = l

class Box<T>
    private var value T

    fn init(v T)
        self.value = v

    public fn get() T
        return self.value

fn main()
    let boxed_widget = Box<Widget>(Widget(7))
    let boxed_gadget = Box<Gadget>(Gadget(\"hi\"))
    let widget = boxed_widget.get()
    let gadget = boxed_gadget.get()
    println(f\"{widget.tag}\")
    println(gadget.label)
",
        "7\nhi",
    );
}

// A collection-backed generic class at a class element: the element has to come
// back out whole, with its own fields readable.
#[test]
fn generic_container_of_class_elements_reads_fields_back() {
    assert_runs_with_output(
        "
use system.collections.queue

class Widget
    public var name String
    fn init(n String)
        self.name = n

fn main()
    var q = Queue<Widget>()
    q.enqueue(Widget(\"first\"))
    q.enqueue(Widget(\"second\"))
    let taken = q.dequeue()
    match taken
        Some(w)
            println(w.name)
        None
            println(\"none\")
",
        "first",
    );
}

// A field read taken straight off a call result (`box.get().tag`) leaves the
// returned value in a temp that nothing releases. Binding the call result to a
// local first is the same program with a holder, and that one is balanced —
// which is what the test above covers. Not generic-specific: a non-generic class
// with the same shape leaks identically.
#[test]
#[ignore = "reading a field directly off a call result never releases the result temp"]
fn generic_class_field_read_off_a_call_result_is_balanced() {
    assert_heap_guard_ok(
        "
class Widget
    public var tag int
    fn init(t int)
        self.tag = t

class Box<T>
    private var value T

    fn init(v T)
        self.value = v

    public fn get() T
        return self.value

fn main()
    let boxed = Box<Widget>(Widget(7))
    println(f\"{boxed.get().tag}\")
",
    );
}
