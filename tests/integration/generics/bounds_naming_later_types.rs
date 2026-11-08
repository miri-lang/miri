// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A type parameter's bound may name a type declared further down the module,
//! as a field type or an `implements` clause may: declaration order does not
//! matter. Every declaration that takes type parameters reads its bounds after
//! every type of the module is known, and a body written above a trait reads
//! that trait's methods. A bound naming a type that exists nowhere is refused
//! where it is written, on every declaration kind.

use super::utils::*;

#[test]
fn a_class_bound_may_name_a_later_trait() {
    assert_heap_guard_output(
        r#"
class Holder<S implements Later>
    var s S
    fn init(s S)
        self.s = s
    public fn read() int
        self.s.n()

trait Later
    fn n() int

class Impl implements Later
    var label String
    fn init()
        self.label = "impl"
    public fn n() int: 3

fn main()
    let h = Holder<Impl>(s: Impl())
    println(f"{h.read()} {h.s.n()}")
"#,
        "3 3",
    );
}

#[test]
fn a_function_bound_may_name_a_later_trait() {
    assert_heap_guard_output(
        r#"
fn get<S implements Later>(s S) int
    return s.n()

trait Later
    fn n() int

class Impl implements Later
    var label String
    fn init()
        self.label = "impl"
    public fn n() int: 4

fn main()
    println(f"{get(Impl())}")
"#,
        "4",
    );
}

#[test]
fn a_method_bound_may_name_a_later_trait() {
    assert_heap_guard_output(
        r#"
class Reader
    var base int
    fn init()
        self.base = 10
    public fn read<S implements Later>(s S) int
        self.base + s.n()

trait Later
    fn n() int

class Impl implements Later
    var label String
    fn init()
        self.label = "impl"
    public fn n() int: 5

fn main()
    println(f"{Reader().read(Impl())}")
"#,
        "15",
    );
}

#[test]
fn a_struct_bound_may_name_a_later_trait() {
    assert_heap_guard_output(
        r#"
struct Boxed<S implements Later>
    item S

fn open<S implements Later>(b Boxed<S>) int
    b.item.n()

trait Later
    fn n() int

class Impl implements Later
    var label String
    fn init()
        self.label = "impl"
    public fn n() int: 6

fn main()
    println(f"{open(Boxed<Impl>(item: Impl()))}")
"#,
        "6",
    );
}

#[test]
fn an_enum_bound_may_name_a_later_trait() {
    assert_heap_guard_output(
        r#"
enum Slot<S implements Later>
    Full(S)
    Empty

    fn value() int
        match self
            Slot.Full(s): s.n()
            Slot.Empty: 0

trait Later
    fn n() int

class Impl implements Later
    var label String
    fn init()
        self.label = "impl"
    public fn n() int: 7

fn main()
    let full = Slot<Impl>.Full(Impl())
    let empty = Slot<Impl>.Empty
    println(f"{full.value()} {empty.value()}")
"#,
        "7 0",
    );
}

#[test]
fn a_trait_parameter_bound_may_name_a_later_trait() {
    assert_heap_guard_output(
        r#"
trait Source<S implements Later>
    fn item() S
    fn total() int: self.item().n()

trait Later
    fn n() int

class Impl implements Later
    var label String
    fn init()
        self.label = "impl"
    public fn n() int: 8

class Box implements Source<Impl>
    var held Impl
    fn init()
        self.held = Impl()
    public fn item() Impl: self.held

fn main()
    println(f"{Box().total()}")
"#,
        "8",
    );
}

#[test]
fn a_class_bound_may_name_a_later_class() {
    assert_heap_guard_output(
        r#"
class Pen<A extends Animal>
    var a A
    fn init(a A)
        self.a = a
    public fn call() String
        self.a.sound()

class Animal
    var name String
    fn init(name String)
        self.name = name
    public fn sound() String: f"{self.name} speaks"

fn main()
    println(Pen<Animal>(a: Animal("cat")).call())
"#,
        "cat speaks",
    );
}

#[test]
fn a_body_above_a_trait_reads_its_methods() {
    assert_heap_guard_output(
        r#"
fn get(s Later) int
    s.n()

trait Later
    fn n() int

class Impl implements Later
    var label String
    fn init()
        self.label = "impl"
    public fn n() int: 9

fn main()
    println(f"{get(Impl())}")
"#,
        "9",
    );
}

#[test]
fn a_class_above_a_trait_it_implements_must_write_its_methods() {
    assert_compiler_error(
        r#"
class Impl implements Later
    var x int
    fn init()
        self.x = 1

trait Later
    fn n() int

fn main()
    println(f"{Impl().x}")
"#,
        "Class 'Impl' must implement method 'n' from trait 'Later'",
    );
}

#[test]
fn a_type_argument_missing_a_later_bound_is_refused() {
    assert_compiler_error(
        r#"
class Holder<S implements Later>
    var s S
    fn init(s S)
        self.s = s

trait Later
    fn n() int

fn main()
    let h = Holder<int>(s: 1)
    println(f"{h.s}")
"#,
        "does not satisfy constraint implements Later",
    );
}

#[test]
fn a_class_bound_naming_no_type_is_refused() {
    assert_compiler_error(
        r#"
class Holder<S implements Missing>
    var s S
    fn init(s S)
        self.s = s

fn main()
    println("x")
"#,
        "Unknown type: Missing",
    );
}

#[test]
fn a_function_bound_naming_no_type_is_refused() {
    assert_compiler_error(
        r#"
fn get<S implements Missing>(s S) int
    0

fn main()
    println("x")
"#,
        "Unknown type: Missing",
    );
}

#[test]
fn a_method_bound_naming_no_type_is_refused() {
    assert_compiler_error(
        r#"
class Reader
    var base int
    fn init()
        self.base = 10
    public fn read<S implements Missing>(s S) int
        self.base

fn main()
    println("x")
"#,
        "Unknown type: Missing",
    );
}

#[test]
fn a_struct_bound_naming_no_type_is_refused() {
    assert_compiler_error(
        r#"
struct Boxed<S implements Missing>
    item S

fn main()
    println("x")
"#,
        "Unknown type: Missing",
    );
}

#[test]
fn an_enum_bound_naming_no_type_is_refused() {
    assert_compiler_error(
        r#"
enum Slot<S implements Missing>
    Full(S)
    Empty

fn main()
    println("x")
"#,
        "Unknown type: Missing",
    );
}

#[test]
fn a_trait_parameter_bound_naming_no_type_is_refused() {
    assert_compiler_error(
        r#"
trait Source<S implements Missing>
    fn item() S

fn main()
    println("x")
"#,
        "Unknown type: Missing",
    );
}

#[test]
fn a_bound_in_an_imported_module_may_name_a_later_trait() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.holders.{Holder, Later}\n",
                    "\n",
                    "class Impl implements Later\n",
                    "    var label String\n",
                    "    fn init()\n",
                    "        self.label = \"impl\"\n",
                    "    public fn n() int: 2\n",
                    "\n",
                    "fn main()\n",
                    "    println(f\"{Holder<Impl>(s: Impl()).read()}\")\n",
                ),
            ),
            (
                "holders.mi",
                concat!(
                    "public class Holder<S implements Later>\n",
                    "    var s S\n",
                    "    fn init(s S)\n",
                    "        self.s = s\n",
                    "    public fn read() int\n",
                    "        self.s.n()\n",
                    "\n",
                    "public trait Later\n",
                    "    fn n() int\n",
                ),
            ),
        ],
        "2",
    );
}

#[test]
fn a_trait_may_extend_a_later_trait_and_name_a_later_class() {
    assert_heap_guard_output(
        r#"
trait A extends B
    fn f() int
    fn g() Later: Later()

trait B
    fn h() int

class Later implements A
    fn init()
        let _x = 1
    public fn f() int: 1
    public fn h() int: 2

fn main()
    println(f"{Later().f()} {Later().g().h()}")
"#,
        "1 2",
    );
}

#[test]
fn a_trait_signature_may_name_a_later_alias_and_struct() {
    assert_heap_guard_output(
        r#"
trait Tr
    fn a() Num
    fn b(x Pt) int
    fn c() Pt?: None

type Num is int

struct Pt
    x int

class Z implements Tr
    fn init()
        let _x = 1
    public fn a() int: 1
    public fn b(x Pt) int: x.x

fn main()
    println(f"{Z().a()} {Z().b(Pt(x: 3))}")
"#,
        "1 3",
    );
}

#[test]
fn a_trait_declared_twice_is_refused() {
    assert_compiler_error(
        r#"
trait X
    fn a() int

trait X
    fn b() int

fn main()
    println("x")
"#,
        "Type 'X' is already defined",
    );
}
