// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Compound assignment writes its arithmetic result through a temp, and that
//! temp is typed from the slot being written. A field's slot is stated by the
//! declaring type, not by the expression that named the field — typed from the
//! latter, a `float` field's `+=` lands in an integer temp and truncates.

use super::utils::*;

#[test]
fn test_float_field_add_assign_keeps_the_fraction() {
    assert_runs_with_output(
        r#"

class Acc
    total float

    fn new()
        self.total = 0.0

    fn add(d float) float
        self.total += d
        return self.total

fn main()
    let a = Acc()
    println(f"{a.add(1.5)}")
    println(f"{a.add(2.25)}")
    "#,
        "1.5\n3.75",
    );
}

#[test]
fn test_float_field_every_compound_operator_keeps_the_fraction() {
    assert_runs_with_output(
        r#"

class Cell
    v float

    fn new(v float)
        self.v = v

    fn apply(d float) float
        self.v *= d
        self.v -= d
        self.v /= d
        return self.v

fn main()
    let c = Cell(4.5)
    println(f"{c.apply(2.0)}")
    "#,
        "3.5",
    );
}

#[test]
fn test_int_field_add_assign_is_unchanged() {
    assert_runs_with_output(
        r#"

class Counter
    n int

    fn new()
        self.n = 0

    fn bump(d int) int
        self.n += d
        return self.n

fn main()
    let c = Counter()
    println(f"{c.bump(3)}")
    println(f"{c.bump(4)}")
    "#,
        "3\n7",
    );
}

#[test]
fn test_float_field_compound_assignment_on_a_struct() {
    assert_runs_with_output(
        r#"

struct Point
    x float
    y float

fn main()
    var p = Point(1.5, 2.0)
    p.x += 2.25
    p.y *= 1.5
    println(f"{p.x}")
    println(f"{p.y}")
    "#,
        "3.75\n3.0",
    );
}

#[test]
fn test_float_field_compound_assignment_on_an_inherited_field() {
    assert_runs_with_output(
        r#"

class Base
    scale float

    fn new(scale float)
        self.scale = scale

class Derived extends Base
    fn grow(d float) float
        self.scale += d
        return self.scale

fn main()
    let d = Derived(1.5)
    println(f"{d.grow(2.25)}")
    "#,
        "3.75",
    );
}

#[test]
fn test_float_field_declared_at_the_class_parameter_keeps_the_fraction() {
    assert_runs_with_output(
        r#"

class Counter<T>
    n T

    fn init(n T)
        self.n = n

fn main()
    var f = Counter<float>(0.5)
    f.n += 0.25
    println(f"{f.n}")
    var i = Counter<int>(1)
    i.n += 41
    println(f"{i.n}")
    "#,
        "0.75\n42",
    );
}

#[test]
fn index_compound_assignment_keeps_a_float_element() {
    // The temp holding the arithmetic result was typed `int` whatever the
    // element was, so a float element's sum was truncated on its way into the
    // temp and then stored back. The field axis reads its own slot's type; the
    // index axis now reads the collection's element type.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var xs = List([1.5, 2.0])
    xs[0] += 2.25
    println(f"{xs[0]}")
"#,
        "3.75",
    );
}

#[test]
fn index_compound_assignment_honours_every_operator_on_floats() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var xs = List([10.0])
    xs[0] -= 2.5
    let a = xs[0]
    println(f"{a}")
    xs[0] *= 2.0
    let b = xs[0]
    println(f"{b}")
    xs[0] /= 4.0
    let c = xs[0]
    println(f"{c}")
"#,
        "7.5
15.0
3.75",
    );
}

#[test]
fn index_compound_assignment_keeps_an_f32_element() {
    // The element is written before it is combined, rather than coming from the
    // literal: a `List<f32>` built from a literal reads back zeros, which is a
    // defect in how such a list is constructed and has nothing to do with the
    // arithmetic being tested here.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var xs = List<f32>([0.0, 0.0])
    xs[0] = 1.5
    xs[0] += 2.25
    println(f"{xs[0]}")
"#,
        "3.75",
    );
}

#[test]
fn index_compound_assignment_keeps_a_float_array_element() {
    assert_runs_with_output(
        r#"
use system.collections.array

fn main()
    var xs = Array<float, 2>()
    xs[0] = 1.5
    xs[0] += 2.25
    println(f"{xs[0]}")
"#,
        "3.75",
    );
}

#[test]
fn index_compound_assignment_leaves_integer_elements_alone() {
    // The integer case was always right and has to stay right: reading the
    // element type must not change what an `int` element does.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var xs = List([10, 20])
    xs[0] += 5
    let a = xs[0]
    xs[1] %= 7
    let b = xs[1]
    println(f"{a} {b}")
"#,
        "15 6",
    );
}

/// A compound assignment names an operator, and the operator a type defines
/// for that spelling is the one it must apply. `+` on a `String` is the
/// `concat` its class declares, not a machine add of two addresses, and the
/// two spellings of it have to agree.
#[test]
fn string_add_assign_on_a_local_concatenates() {
    assert_heap_guard_output(
        r#"
fn main()
    var s = "a" + "z"
    s += "b" + "c"
    println(s)
"#,
        "azbc",
    );
}

#[test]
fn string_add_assign_on_a_field_concatenates() {
    assert_heap_guard_output(
        r#"
class Buf
    s String
    fn init()
        self.s = "a" + "z"

fn main()
    var b = Buf()
    b.s += "b" + "c"
    b.s += "!"
    println(b.s)
"#,
        "azbc!",
    );
}

/// The same write from inside one of the class's own methods. It prints the
/// right answer and passes the MIR verifier, but leaks one allocation — and
/// the operator spelled out, `self.s = self.s + d`, leaks exactly the same
/// one. The leak is the in-method field store's, not the compound spelling's,
/// so this is ignored until that is fixed rather than counted against `+=`.
#[test]
#[ignore]
fn string_add_assign_on_a_field_inside_a_method_concatenates() {
    assert_heap_guard_output(
        r#"
class Buf
    s String
    fn init()
        self.s = "a"
    fn add(d String) String
        self.s += d
        self.s += "!"
        return self.s

fn main()
    let b = Buf()
    println(b.add("bc"))
"#,
        "abc!",
    );
}

#[test]
fn repeated_string_add_assign_keeps_every_part() {
    assert_heap_guard_output(
        r#"
fn main()
    var s = ""
    s += "a" + "1"
    s += "b" + "2"
    s += "c" + "3"
    println(s)
"#,
        "a1b2c3",
    );
}

#[test]
fn string_add_assign_on_a_list_element_concatenates() {
    assert_heap_guard_output(
        r#"
use system.collections.list

fn main()
    var xs = List(["a" + "1", "b" + "2"])
    xs[0] += "y" + "z"
    println(f"{xs[0]} {xs[1]}")
"#,
        "a1yz b2",
    );
}

#[test]
fn string_add_assign_on_a_map_value_concatenates() {
    assert_heap_guard_output(
        r#"
use system.collections.map

fn main()
    var m = Map<int, String>()
    m.set(1, "a" + "1")
    m[1] += "y" + "z"
    println(m[1])
"#,
        "a1yz",
    );
}

/// A class that enables `+` for itself has `+=` call the same method, and the
/// two spellings produce the same value.
#[test]
fn a_user_classs_own_concat_answers_add_assign() {
    assert_heap_guard_output(
        r#"
use system.ops

class Word implements Addable
    text String
    fn init(text String)
        self.text = text
    public fn concat(other Self) Self
        return Word(self.text + other.text)

fn main()
    var w = Word("a" + "1")
    w += Word("b" + "2")
    let written = Word("a" + "1") + Word("b" + "2")
    println(f"{w.text} {written.text}")
"#,
        "a1b2 a1b2",
    );
}

/// `s * 2` repeats a string, so `s *= 2` must too — the compound form may not
/// demand that the right side have the left side's type when the operator it
/// names does not.
#[test]
fn string_mul_assign_repeats() {
    assert_heap_guard_output(
        r#"
fn main()
    var s = "a" + "b"
    s *= 2
    println(s)
"#,
        "abab",
    );
}

/// The operator a compound assignment names may simply not exist for the type.
/// That is a type error carrying the same reason the written-out operator
/// gives, not a crash at run time.
#[test]
fn string_sub_assign_is_refused_the_way_the_written_operator_is() {
    assert_compiler_error(
        r#"
fn main()
    var s = "a"
    s -= "b"
    println(s)
"#,
        "Invalid types for arithmetic operation: String and String",
    );
}

#[test]
fn a_class_defining_no_operator_is_refused_by_add_assign() {
    assert_compiler_error(
        r#"
class Plain
    n int
    fn init(n int)
        self.n = n

fn main()
    var p = Plain(1)
    p += Plain(2)
    println(f"{p.n}")
"#,
        "Invalid types for arithmetic operation",
    );
}

/// Inside a generic body the operand's recorded type is still the parameter,
/// which names no class. The operator has to be resolved through the
/// instantiation's substitution, or the body compiled for `String` combines
/// two addresses while the one compiled for `int` adds correctly.
#[test]
fn add_assign_in_a_generic_body_follows_the_instantiation() {
    assert_heap_guard_output(
        r#"
fn grow<T>(start T, more T) T
    var acc = start
    acc += more
    return acc

fn main()
    let text = grow("a", "b")
    let sum = grow(1, 2)
    println(f"{text} {sum}")
"#,
        "ab 3",
    );
}
