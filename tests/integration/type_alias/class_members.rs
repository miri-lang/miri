// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A class's method signatures are resolved while declarations are collected,
//! before any body is checked, so an alias has to be known by then for a
//! signature to spell it.

use super::utils::*;

#[test]
fn an_alias_types_a_method_parameter_and_result() {
    assert_runs_with_output(
        r#"
use system.io

type Id is int

class Q
    var n int
    fn bump(i Id) Id
        return self.n + i

fn main()
    let q = Q(n: 2)
    println(f"{q.bump(3)}")
"#,
        "5\n",
    );
}

#[test]
fn an_alias_declared_below_the_class_types_its_method() {
    assert_runs_with_output(
        r#"
use system.io

class Q
    var n int
    fn bump(i Id) Id
        return self.n + i

type Id is int

fn main()
    let q = Q(n: 2)
    println(f"{q.bump(3)}")
"#,
        "5\n",
    );
}

#[test]
fn an_alias_types_an_init_parameter() {
    assert_runs_with_output(
        r#"
use system.io

type Id is int

class Q
    var n int
    fn init(start Id)
        self.n = start * 10

fn main()
    let q = Q(start: 4)
    println(f"{q.n}")
"#,
        "40\n",
    );
}

#[test]
fn an_alias_naming_a_class_declared_later_types_a_method() {
    assert_runs_with_output(
        r#"
use system.io

type Box is Crate

class Q
    var n int
    fn keep(b Box) Box
        return b

class Crate
    var v int

fn main()
    let q = Q(n: 6)
    println(f"{q.keep(Crate(v: 7)).v}")
"#,
        "7\n",
    );
}

#[test]
fn an_alias_of_an_imported_class_types_a_method() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.crates.{Crate}\n",
                    "\n",
                    "type Box is Crate\n",
                    "\n",
                    "class Q\n",
                    "    var n int\n",
                    "    fn open(b Box) int\n",
                    "        return b.v + self.n\n",
                    "\n",
                    "fn main()\n",
                    "    let q = Q(n: 1)\n",
                    "    println(f\"{q.open(Crate(v: 8))}\")\n",
                ),
            ),
            (
                "crates.mi",
                concat!("public class Crate\n", "    public var v int\n"),
            ),
        ],
        "9\n",
    );
}

#[test]
fn an_imported_class_whose_method_spells_its_module_alias_runs() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.sig.{Q}\n",
                    "\n",
                    "fn main()\n",
                    "    let q = Q(n: 2)\n",
                    "    println(f\"{q.bump(3)}\")\n",
                ),
            ),
            (
                "sig.mi",
                concat!(
                    "public type Id is int\n",
                    "\n",
                    "public class Q\n",
                    "    public var n int\n",
                    "    public fn bump(i Id) Id\n",
                    "        return self.n + i\n",
                ),
            ),
        ],
        "5\n",
    );
}

#[test]
fn an_alias_with_an_unknown_target_is_still_reported_at_the_target() {
    // The early registration tries the target quietly and gives up on it;
    // the error must still come from the `type` statement itself.
    assert_compiler_error(
        r#"
type Id is Nowhere

class Q
    var n int
    fn bump(i Id) int
        return self.n

fn main()
    let q = Q(n: 2)
"#,
        "Unknown type: Nowhere",
    );
}
