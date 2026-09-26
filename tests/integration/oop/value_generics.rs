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

/// A class naming its own type in a signature works for a purely type-generic
/// class, and a value parameter must not change that.
///
/// The sizes on both sides are the same here deliberately: nothing yet
/// compares the value a size generic carries, so a mismatched pair is accepted
/// and would make this assert the wrong thing. The parameter stands for
/// a value, so naming the class at its own parameters puts it in a type
/// position, and the marker it is carried through substitution as must never
/// reach the reader.
#[test]
fn a_value_generic_class_names_its_own_type_written_out() {
    assert_runs_with_output(
        r#"
use system.collections.array

class Wrap<T, Size>
    data Array<T, Size>

    fn init(data Array<T, Size>)
        self.data = data

    public fn first_of(other Wrap<T, Size>) T
        return other.data[0]

fn main()
    let w = Wrap<int, 3>([1, 2, 3])
    let v = Wrap<int, 3>([5, 6, 7])
    println(f"{w.first_of(v)}")
"#,
        "5",
    );
}

#[test]
fn a_value_generic_class_names_its_own_type_as_self() {
    assert_runs_with_output(
        r#"
use system.collections.array

class Wrap<T, Size>
    data Array<T, Size>

    fn init(data Array<T, Size>)
        self.data = data

    public fn first_of(other Self) T
        return other.data[0]

fn main()
    let w = Wrap<int, 3>([1, 2, 3])
    let v = Wrap<int, 3>([5, 6, 7])
    println(f"{w.first_of(v)}")
"#,
        "5",
    );
}

#[test]
fn a_value_generic_class_names_its_own_type_bare() {
    assert_runs_with_output(
        r#"
use system.collections.array

class Wrap<T, Size>
    data Array<T, Size>

    fn init(data Array<T, Size>)
        self.data = data

    public fn first_of(other Wrap) T
        return other.data[0]

fn main()
    let w = Wrap<int, 3>([1, 2, 3])
    let v = Wrap<int, 3>([5, 6, 7])
    println(f"{w.first_of(v)}")
"#,
        "5",
    );
}

/// A value-generic class with no self-typed member was never affected, and has
/// to stay that way.
#[test]
fn a_value_generic_class_without_a_self_typed_member_still_reads_its_field() {
    assert_runs_with_output(
        r#"
use system.collections.array

class Wrap<T, Size>
    data Array<T, Size>

    fn init(data Array<T, Size>)
        self.data = data

    public fn first() T
        return self.data[0]

fn main()
    let w = Wrap<int, 3>([1, 2, 3])
    println(f"{w.first()}")
"#,
        "1",
    );
}

/// The same signature on a class with only type parameters, which is what the
/// value-generic spelling has to agree with.
#[test]
fn a_type_generic_class_names_its_own_type_the_same_way() {
    assert_runs_with_output(
        r#"
use system.collections.list

class Holder<T>
    data List<T>

    fn init(data List<T>)
        self.data = data

    public fn first_of(other Holder<T>) T
        return other.data[0]

fn main()
    var one = List<int>()
    one.push(1)
    var two = List<int>()
    two.push(5)
    let w = Holder<int>(one)
    let v = Holder<int>(two)
    println(f"{w.first_of(v)}")
"#,
        "5",
    );
}

/// The marker a value generic travels as inside the substitution map names no
/// type anyone wrote. A diagnostic about such a type must show the value the
/// reader wrote, not the compiler's spelling for it.
#[test]
fn a_diagnostic_about_a_value_generic_type_shows_the_value_not_the_marker() {
    assert_compiler_error(
        r#"
use system.collections.array

class Wrap<T, Size>
    data Array<T, Size>

    fn init(data Array<T, Size>)
        self.data = data

    public fn first_of(other Wrap<T, Size>) T
        return other.data[0]

fn main()
    let w = Wrap<int, 3>([1, 2, 3])
    println(f"{w.first_of(7)}")
"#,
        "expected Wrap<int, 3>, got int",
    );
}

/// A value-generic class's own `init` is compiled per instantiation: the
/// argument crosses into a body typed at the instance's `T`, so a string is
/// stored as a string rather than through the body shared by every
/// instantiation.
#[test]
fn a_value_generic_class_init_stores_a_managed_argument_at_its_instantiation() {
    assert_heap_guard_output(
        r#"
use system.io

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v

fn main()
    let b = Buf<String, 2>("hi" + "x")
    println(b.get())
"#,
        "hix",
    );
}

/// A float handed to a value-generic class's `init` crosses the call at a
/// float's width, statically and behind a trait receiver alike.
#[test]
fn a_value_generic_class_init_stores_a_float_at_its_width() {
    assert_heap_guard_output(
        r#"
use system.io

trait Op<T>
    fn get() T

class W<T, Size> implements Op<T>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v

fn main()
    let w = W<float, 2>(3.5)
    println(f"{w.get()}")
    let o Op<float> = W<float, 2>(2.5)
    println(f"{o.get()}")
"#,
        "3.5\n2.5",
    );
}

/// An instance a value-generic class builds of itself at a fixed value inside
/// a method reached through a trait runs its own `init`. The instances are
/// held at their class types: a trait-typed binding releases none of its
/// instance's fields yet.
#[test]
fn a_value_generic_instance_built_inside_a_dispatched_method_runs_its_own_init() {
    assert_heap_guard_output(
        r#"
use system.io

trait Op
    fn depth(n int) int

class Wrap<T>
    v T
    fn init(v T)
        self.v = v

class Buf<T, Size> implements Op
    v T
    fn init(v T)
        self.v = v
    fn depth(n int) int
        if n == 0
            return 0
        let w = Wrap<T>(self.v)
        let a = Buf<T, 2>(w.v)
        return 1 + a.depth(n - 1)

fn run(o Op) int
    return o.depth(4)

fn main()
    let b = Buf<String, 1>("hi" + "x")
    println(f"{run(b)}")
"#,
        "4",
    );
}

/// A value argument built from a value known only while the program runs names
/// no instantiation, inside a generic body as at the top level: it is refused
/// where it is written.
#[test]
fn a_value_argument_over_a_runtime_value_inside_a_generic_body_is_refused() {
    assert_compiler_error(
        r#"
use system.io

fn seven() int
    return 7

trait Op<T>
    fn depth(n int) T

class Buf<T, Size> implements Op<T>
    v T
    fn init(v T)
        self.v = v
    fn depth(n int) T
        if n == 0
            return self.v
        let m = seven()
        let a Op<T> = Buf<T, Size + m>(self.v)
        return a.depth(n - 1)

fn main()
    let o Op<float> = Buf<float, 2>(3.25)
    println(f"{o.depth(1)}")
"#,
        "MER_TYP_076",
    );
}

/// The refusal names the runtime value, whatever the element type.
#[test]
fn a_value_argument_over_a_runtime_value_names_the_value() {
    assert_compiler_error(
        r#"
use system.io

fn seven() int
    return 7

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v

fn main()
    let m = seven()
    let p = Buf<String, 2 + m>("a" + "b")
    println(p.get())
"#,
        "`m` is not a compile-time constant",
    );
}

/// A top-level value argument over a runtime value is refused rather than
/// reaching code generation with no size.
#[test]
fn a_top_level_value_argument_over_a_runtime_value_is_refused() {
    assert_compiler_error(
        r#"
use system.io

fn seven() int
    return 7

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v

fn main()
    let m = seven()
    let p = Buf<float, 2 + m>(4.5)
    println(f"{p.get()}")
"#,
        "MER_TYP_076",
    );
}

/// A call is no more a compile-time constant than the binding holding its
/// result.
#[test]
fn a_value_argument_calling_a_function_is_refused() {
    assert_compiler_error(
        r#"
fn seven() int
    return 7

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v

fn main()
    let p = Buf<float, 1 + seven()>(4.5)
"#,
        "MER_TYP_076",
    );
}

/// A named `const`, an immutable binding of a literal and a value parameter of
/// the enclosing class are all compile-time constants.
#[test]
fn a_value_argument_over_constants_and_value_parameters_runs() {
    assert_heap_guard_output(
        r#"
use system.io

const K = 3

class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v
    fn get() T
        return self.v
    fn grown() T
        let b = Buf<T, Size + K>(self.v)
        return b.get()

fn main()
    let n = 2
    let b = Buf<String, K + n>("a" + "b")
    println(b.grown())
    let f = Buf<float, 1 + K>(1.5)
    println(f"{f.grown()}")
"#,
        "ab\n1.5",
    );
}
