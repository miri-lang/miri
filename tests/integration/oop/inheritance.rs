// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_inherited_method_basic() {
    // Dog extends Animal but does not override speak — must resolve to Animal_speak
    assert_runs_with_output(
        r#"

class Animal
    var name String
    fn speak()
        println("animal")

class Dog extends Animal
    var breed String

fn main()
    let d = Dog()
    d.speak()
    "#,
        "animal",
    );
}

#[test]
fn test_overridden_method_uses_child_impl() {
    // Dog overrides speak — must call Dog_speak, not Animal_speak
    assert_runs_with_output(
        r#"

class Animal
    var name String
    fn speak()
        println("animal")

class Dog extends Animal
    var breed String
    fn speak()
        println("dog")

fn main()
    let d = Dog()
    d.speak()
    "#,
        "dog",
    );
}

#[test]
fn test_multi_level_inheritance_calls_grandparent() {
    // Poodle extends Dog extends Animal — speak only on Animal
    assert_runs_with_output(
        r#"

class Animal
    var name String
    fn speak()
        println("animal")

class Dog extends Animal
    var breed String

class Poodle extends Dog
    var size String

fn main()
    let p = Poodle()
    p.speak()
    "#,
        "animal",
    );
}

#[test]
fn test_override_at_middle_level_of_chain() {
    // Poodle extends Dog extends Animal — Dog overrides speak, Poodle does not
    assert_runs_with_output(
        r#"

class Animal
    var name String
    fn speak()
        println("animal")

class Dog extends Animal
    var breed String
    fn speak()
        println("dog")

class Poodle extends Dog
    var size String

fn main()
    let p = Poodle()
    p.speak()
    "#,
        "dog",
    );
}

#[test]
fn test_inherited_method_with_return_value() {
    // Inherited method that returns int
    assert_runs_with_output(
        r#"

class Shape
    var id int
    fn area() int
        42

class Circle extends Shape
    var radius int

fn main()
    let c = Circle()
    println(f"{c.area()}")
    "#,
        "42",
    );
}

#[test]
fn test_both_own_and_inherited_methods() {
    // Dog has its own method AND inherits speak from Animal
    assert_runs_with_output(
        r#"

class Animal
    var name String
    fn speak()
        println("animal")

class Dog extends Animal
    var breed String
    fn fetch()
        println("fetching")

fn main()
    let d = Dog()
    d.speak()
    d.fetch()
    "#,
        "animal\nfetching",
    );
}

#[test]
fn test_inherited_init_no_own_fields() {
    // Dog has no own fields; constructor must detect and call Animal_init
    assert_runs_with_output(
        r#"

class Animal
    var name String
    fn init(n String)
        self.name = n

class Dog extends Animal

fn main()
    let d = Dog(n: "Rex")
    println(d.name)
    "#,
        "Rex",
    );
}

#[test]
fn test_parent_method_reads_inherited_field() {
    // Animal.speak reads self.name; must use correct offset when called on Dog instance
    assert_runs_with_output(
        r#"

class Animal
    var name String
    fn init(n String)
        self.name = n
    fn speak()
        println(self.name)

class Dog extends Animal

fn main()
    let d = Dog(n: "Rex")
    d.speak()
    "#,
        "Rex",
    );
}

#[test]
fn test_subclass_method_reads_inherited_field() {
    // DoubleCounter.doubled() reads self.count which is an inherited field
    assert_runs_with_output(
        r#"

class Counter
    var count int
    fn init(c int)
        self.count = c

class DoubleCounter extends Counter
    fn doubled() int
        self.count * 2

fn main()
    let dc = DoubleCounter(c: 5)
    println(f"{dc.doubled()}")
    "#,
        "10",
    );
}

#[test]
fn test_extends_generic_base_substitutes_param_int() {
    // `class IntStack extends Stack<int>` overriding `push(value T)` with
    // `push(value int)` must be accepted: T = int after substitution.
    assert_runs_with_output(
        r#"

class Stack<T>
    var marker int
    fn init(m int)
        self.marker = m
    fn set_marker(value T)
        self.marker = 0

class IntStack extends Stack<int>
    fn set_marker(value int)
        self.marker = value

fn main()
    let s = IntStack(m: 0)
    s.set_marker(42)
    println(f"{s.marker}")
    "#,
        "42",
    );
}

#[test]
fn test_extends_generic_base_rejects_wrong_param() {
    // After substituting Stack<int>, the inherited `push(value T)` becomes
    // `push(value int)`. An override `push(value String)` must be rejected
    // with the substituted expected type, not the bare `T`.
    assert_compiler_error(
        r#"

class Stack<T>
    var top T
    fn init(t T)
        self.top = t
    fn push(value T)
        self.top = value

class IntStack extends Stack<int>
    fn push(value String)
        println(value)

fn main()
    let s = IntStack(t: 5)
    "#,
        "Method 'push' has incompatible parameter type for 'value' (position 1): expected int, got String",
    );
}

#[test]
fn test_extends_generic_base_rejects_wrong_return() {
    // `class StringBox extends Box<String>` overriding `unwrap() T`
    // with `unwrap() int` must be rejected; substituted return type is String.
    assert_compiler_error(
        r#"

class Box<T>
    var item T
    fn init(i T)
        self.item = i
    fn unwrap() T
        return self.item

class StringBox extends Box<String>
    fn unwrap() int
        return 0

fn main()
    let b = StringBox(i: "hi")
    "#,
        "Method 'unwrap' has incompatible return type: expected String, got int",
    );
}

#[test]
fn test_extends_generic_chain_composition_propagates() {
    // Chain composition: `C extends B<int>`, `B<U> extends A<U>`. A's `T` must
    // resolve to `int` after composing B's substitution. Concrete-typed override
    // in C must be accepted.
    assert_runs_with_output(
        r#"

class A<V>
    fn ancestor_method(value V) int
        return 1

class B<U> extends A<U>

class C extends B<int>
    fn ancestor_method(value int) int
        return value

fn main()
    let c = C()
    println(f"{c.ancestor_method(42)}")
    "#,
        "42",
    );
}

#[test]
fn test_extends_generic_chain_composition_works_out_of_order() {
    // Same chain as above, but child declared before its ancestors. Pass 1b
    // must populate `base_class_args` for shells so descendants whose
    // `check_class` runs first can compose substitutions through intermediate
    // generic ancestors.
    assert_runs_with_output(
        r#"

class C extends B<int>
    fn ancestor_method(value int) int
        return value

class B<U> extends A<U>

class A<V>
    fn ancestor_method(value V) int
        return 1

fn main()
    let c = C()
    println(f"{c.ancestor_method(42)}")
    "#,
        "42",
    );
}

#[test]
fn test_extends_generic_chain_composition_rejects_mismatch() {
    // After composing B's `<int>` substitution into A, the inherited
    // `ancestor_method(value V)` becomes `ancestor_method(value int)`. An
    // override declaring `value String` must be rejected with the substituted
    // expected type, not the bare `V`.
    assert_compiler_error(
        r#"
class C extends B<int>
    fn ancestor_method(value String)
        return

class B<U> extends A<U>

class A<V>
    fn ancestor_method(value V)
        return
    "#,
        "Method 'ancestor_method' has incompatible parameter type for 'value' (position 1): expected int, got String",
    );
}

#[test]
fn test_field_layout_base_fields_before_derived() {
    // Base class fields must come before derived class fields in memory layout.
    // Dog has own field `breed`; Animal has `name`. Full layout: [name, breed].
    // Animal.init sets self.name; Dog.init calls super.init then sets self.breed.
    assert_runs_with_output(
        r#"

class Animal
    var name String
    fn init(n String)
        self.name = n

class Dog extends Animal
    var breed String
    fn init(n String, b String)
        super.init(n)
        self.breed = b
    fn describe()
        println(self.name)
        println(self.breed)

fn main()
    let d = Dog(n: "Rex", b: "Lab")
    d.describe()
    "#,
        "Rex\nLab",
    );
}

#[test]
fn test_self_field_read_modify_write_scalar() {
    // Regression: read-modify-write on a scalar field inside a non-constructor
    // method body used to crash because the BinaryOp temp inherited the base
    // local's (class) type instead of the projected scalar type, causing
    // Perceus to DecRef an integer value as if it were a managed pointer.
    assert_runs_with_output(
        r#"

class Counter
    var count int
    fn init(c int)
        self.count = c
    fn double()
        self.count = self.count * 2
    fn inc()
        self.count = self.count + 1

fn main()
    let c = Counter(c: 5)
    c.double()
    c.inc()
    println(f"{c.count}")
    "#,
        "11",
    );
}

#[test]
fn test_self_field_unary_negate_scalar() {
    // Regression: the UnaryOp result temp inherited the base local's (class)
    // type instead of the projected scalar type when negating `self.field`,
    // causing Perceus to mis-type the result and the program to crash.
    assert_runs_with_output(
        r#"

class Signed
    var v int
    fn init(x int)
        self.v = x
    fn flip()
        self.v = -self.v

fn main()
    let s = Signed(x: 7)
    s.flip()
    println(f"{s.v}")
    "#,
        "-7",
    );
}

#[test]
fn test_override_drops_out_modifier_is_rejected() {
    // ABI safety: parent declares `out`, child override drops it. A vtable
    // caller would box the scalar into a stack slot while the child callee
    // treats the param as a plain int → silent memory corruption. Must error.
    assert_compiler_error(
        r#"
abstract class Base
    fn inc(n out int)

class Child extends Base
    fn inc(n int)
        let _ = n
    "#,
        "incompatible 'out' modifier",
    );
}
