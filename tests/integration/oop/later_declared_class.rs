// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A class's fields are known before any body is checked, so a body written
//! above the class — a method of an earlier class or a free function — can
//! read, store and construct with them.

use super::utils::*;

#[test]
fn an_earlier_method_reads_a_field_of_a_later_class_parameter() {
    assert_runs_with_output(
        r#"
use system.io

class Q
    var n int
    fn open(b Crate) int
        return b.v + self.n

class Crate
    var v int

fn main()
    let q = Q(n: 6)
    println(f"{q.open(Crate(v: 1))}")
"#,
        "7\n",
    );
}

#[test]
fn an_earlier_method_constructs_a_later_class_by_field_name() {
    assert_runs_with_output(
        r#"
use system.io

class Q
    var n int
    fn make() Crate
        return Crate(v: self.n + 1)

class Crate
    var v int

fn main()
    let q = Q(n: 6)
    println(f"{q.make().v}")
"#,
        "7\n",
    );
}

#[test]
fn an_earlier_method_stores_into_a_field_of_a_later_class() {
    assert_runs_with_output(
        r#"
use system.io

class Q
    var n int
    fn fill() int
        var c = Crate(v: 0)
        c.v = self.n * 2
        return c.v

class Crate
    var v int

fn main()
    let q = Q(n: 4)
    println(f"{q.fill()}")
"#,
        "8\n",
    );
}

#[test]
fn two_classes_each_use_the_other_in_a_method_body() {
    assert_runs_with_output(
        r#"
use system.io

class Ping
    var a int
    fn bounce(p Pong) int
        return p.b + Pong(b: self.a).b

class Pong
    var b int
    fn bounce(p Ping) int
        return p.a + Ping(a: self.b).a

fn main()
    let x = Ping(a: 2)
    let y = Pong(b: 5)
    println(f"{x.bounce(y)} {y.bounce(x)}")
"#,
        "7 7\n",
    );
}

#[test]
fn a_free_function_above_a_class_reads_and_constructs_it() {
    assert_runs_with_output(
        r#"
use system.io

fn total(c Crate) int
    return c.v + c.w

fn make() Crate
    return Crate(v: 3, w: 4)

fn main()
    println(f"{total(make())}")

class Crate
    var v int
    let w int
"#,
        "7\n",
    );
}

#[test]
fn a_field_typed_by_its_initializer_is_known_to_an_earlier_body() {
    assert_runs_with_output(
        r#"
use system.io

class Q
    fn open(b Crate) String
        return f"{b.v} {b.label}"

class Crate
    var v = 7
    var label = "crate"

fn main()
    let q = Q()
    println(q.open(Crate(v: 7, label: "crate")))
"#,
        "7 crate\n",
    );
}

#[test]
fn an_earlier_method_reads_a_field_of_a_later_generic_class() {
    assert_runs_with_output(
        r#"
use system.io

class Q
    fn open(b Holder<String>) String
        return b.item

class Holder<T>
    var item T

fn main()
    let q = Q()
    println(q.open(Holder<String>(item: "held")))
"#,
        "held\n",
    );
}

#[test]
fn a_field_the_later_class_does_not_declare_is_still_refused() {
    assert_compiler_error(
        r#"
class Q
    fn open(b Crate) int
        return b.w

class Crate
    var v int

fn main()
    let q = Q()
"#,
        "Type 'Crate' has no field or method 'w'",
    );
}

#[test]
fn a_bad_field_type_on_a_later_class_is_reported_once() {
    let code = r#"
class Q
    fn open(b Crate) int
        return 1

class Crate
    var v Missing

fn main()
    let q = Q()
"#;
    assert_compiler_error(code, "Missing");
    let output = crate::utils::miri_check(code).output();
    assert_eq!(output.matches("error[").count(), 1, "{output}");
}

#[test]
fn an_imported_module_method_reads_a_class_declared_later_in_the_module() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.shapes.{Q, Crate}\n",
                    "let q = Q(n: 6)\n",
                    "println(f\"{q.open(Crate(v: 1))} {q.make().v}\")\n",
                ),
            ),
            (
                "shapes.mi",
                concat!(
                    "public class Q\n",
                    "    public var n int\n",
                    "    public fn open(b Crate) int\n",
                    "        return b.v + self.n\n",
                    "    public fn make() Crate\n",
                    "        return Crate(v: self.n + 1)\n",
                    "\n",
                    "public class Crate\n",
                    "    public var v int\n",
                ),
            ),
        ],
        "7 7\n",
    );
}
