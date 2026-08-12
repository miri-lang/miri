// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

// ===== Basic static method functionality =====

#[test]
fn test_static_method_basic() {
    assert_runs_with_output(
        r#"
class Duration
    var millis int

    public static fn from_millis(ms int) Duration
        Duration(millis: ms)

fn main()
    let d = Duration.from_millis(2)
    println(f"{d.millis}")
    "#,
        "2",
    );
}

#[test]
fn test_static_method_returning_string() {
    assert_runs_with_output(
        r#"
class Config
    var name String

    public static fn create(name String) Config
        Config(name: name)

fn main()
    let c = Config.create("test")
    println(c.name)
    "#,
        "test",
    );
}

#[test]
fn test_static_method_returning_managed_value_no_leak() {
    assert_runs_with_output(
        r#"
class StringHolder
    var value String

    public static fn with_value(v String) StringHolder
        StringHolder(value: v)

fn main()
    let s1 = StringHolder.with_value("hello")
    let s2 = StringHolder.with_value("world")
    println(s1.value)
    println(s2.value)
    "#,
        "hello\nworld",
    );
}

#[test]
fn test_multiple_static_methods() {
    assert_runs_with_output(
        r#"
class Math
    public static fn add(a int, b int) int
        a + b

    public static fn multiply(a int, b int) int
        a * b

fn main()
    let sum = Math.add(3, 4)
    let product = Math.multiply(2, 5)
    println(f"{sum}")
    println(f"{product}")
    "#,
        "7\n10",
    );
}

// ===== Visibility tests =====

#[test]
fn test_private_static_method_not_callable_externally() {
    assert_compiler_error(
        r#"
class Helper
    private static fn secret() int
        42

fn main()
    let x = Helper.secret()
    "#,
        "Private",
    );
}

#[test]
fn test_protected_static_method_not_callable_externally() {
    assert_compiler_error(
        r#"
class Parent
    protected static fn internal() int
        10

fn main()
    let x = Parent.internal()
    "#,
        "Protected",
    );
}

// ===== Error cases =====

#[test]
fn test_static_constructor_is_invalid() {
    assert_compiler_error(
        r#"
class Duration
    public static fn init(ms int) Duration
        Duration(millis: ms)
    var millis int
    "#,
        "constructor cannot be static",
    );
}

#[test]
fn test_self_in_static_method_is_error() {
    assert_compiler_error(
        r#"
class Counter
    var count int

    public static fn reset()
        self.count = 0

fn main()
    "#,
        "self",
    );
}

#[test]
fn test_static_combined_with_async_is_error() {
    assert_compiler_error(
        r#"
class Task
    public static async fn run() int
        42
    "#,
        "async",
    );
}

#[test]
fn test_static_combined_with_gpu_is_error() {
    assert_compiler_error(
        r#"
class Kernel
    public static gpu fn compute() int
        42
    "#,
        "gpu",
    );
}

#[test]
fn test_calling_instance_method_on_type_is_error() {
    assert_compiler_error(
        r#"
class Point
    var x int

    fn distance() int
        x

fn main()
    let d = Point.distance()
    "#,
        "instance",
    );
}

#[test]
fn test_calling_static_method_on_instance_is_error() {
    assert_compiler_error(
        r#"
class Factory
    public static fn create() Factory
        Factory()

fn main()
    var f = Factory()
    let f2 = f.create()
    "#,
        "instance",
    );
}

#[test]
fn test_static_in_trait_is_error() {
    assert_compiler_error(
        r#"
trait Buildable
    public static fn build() int
    "#,
        "not allowed in trait",
    );
}

// ===== Inheritance tests =====

#[test]
fn test_static_method_inheritance() {
    assert_runs_with_output(
        r#"
class Base
    public static fn getValue() int
        42

class Derived extends Base

fn main()
    println(f"{Derived.getValue()}")
    "#,
        "42",
    );
}

#[test]
fn test_calling_static_from_parent_through_child() {
    assert_runs_with_output(
        r#"
class Animal
    public static fn species() String
        "creature"

class Dog extends Animal

fn main()
    println(Dog.species())
    "#,
        "creature",
    );
}

// ===== Rejection cases for deferred features =====

#[test]
fn test_static_method_referencing_class_generic_parameter_is_error() {
    assert_compiler_error(
        r#"
class Box<T>
    var v int

    fn init(v int)
        self.v = v

    public static fn wrap(item T) Box<T>
        Box<T>(v: 1)

fn main()
    println("compiled")
    "#,
        "generic parameter",
    );
}

#[test]
fn test_duplicate_instance_and_static_method_names_is_error() {
    assert_compiler_error(
        r#"
class C
    var v int

    fn init(v int)
        self.v = v

    public fn name() int
        self.v

    public static fn name() int
        7

fn main()
    println(f"{C.name()}")
    "#,
        "cannot be both",
    );
}

// ===== Out parameter tests =====

#[test]
fn test_static_method_with_out_parameter() {
    assert_runs_with_output(
        r#"
class Test
    public static fn copy(input int, result out int)
        result = input

fn main()
    var x = 0
    Test.copy(42, x)
    println(f"{x}")
    "#,
        "42",
    );
}

#[test]
fn test_static_method_with_multiple_out_parameters() {
    assert_runs_with_output(
        r#"
class Pair
    public static fn swap(a int, b int, out_a out int, out_b out int)
        out_a = b
        out_b = a

fn main()
    var x = 10
    var y = 20
    Pair.swap(1, 2, x, y)
    println(f"{x}")
    println(f"{y}")
    "#,
        "2\n1",
    );
}

#[test]
fn test_static_method_with_mixed_normal_and_out_params() {
    assert_runs_with_output(
        r#"
class Calc
    public static fn add_and_store(a int, b int, result out int)
        result = a + b

fn main()
    var sum = 0
    Calc.add_and_store(5, 7, sum)
    println(f"{sum}")
    "#,
        "12",
    );
}

#[test]
fn test_static_method_with_out_inherited_from_base() {
    assert_runs_with_output(
        r#"
class Base
    public static fn set_value(val int, result out int)
        result = val * 2

class Derived extends Base

fn main()
    var output = 0
    Derived.set_value(21, output)
    println(f"{output}")
    "#,
        "42",
    );
}

#[test]
fn test_instance_and_static_collision_underlines_method_name() {
    // The caret must cover the method's own name. It previously fell through to
    // the first parameter's type, or to the return type for a method with no
    // parameters, pointing the reader at the wrong token.
    assert_compiler_error(
        r#"
class Registry
    public fn lookup() int
        return 1

    public static fn lookup() int
        return 2
    "#,
        "^^^^^^ Method 'lookup' cannot be both instance and static method",
    );
}

#[test]
fn test_instance_and_static_collision_underlines_name_with_params() {
    // With a parameter present the old fallback underlined the parameter's
    // type; the name span must win regardless of the signature's shape.
    assert_compiler_error(
        r#"
class Registry
    public fn resolve(key int) int
        return key

    public static fn resolve(key int) int
        return key
    "#,
        "^^^^^^^ Method 'resolve' cannot be both instance and static method",
    );
}
