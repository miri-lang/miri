// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A module's declarations can name the types the module imports.
//!
//! An imported module is checked on its own, so the names its `use` lines
//! bring in have to be in reach before its classes' method signatures are
//! resolved, exactly as they are for the program's own file.

use super::utils::*;

/// The module that declares the type the others import.
const OTHER: &str = concat!("public class Foo\n", "    public var v int\n",);

/// A program that builds a `Foo` and hands it to `Q.take`.
const MAIN: &str = concat!(
    "use local.sig.{Q}\n",
    "use local.other.{Foo}\n",
    "\n",
    "fn main()\n",
    "    let q = Q(n: 2)\n",
    "    println(f\"{q.take(Foo(v: 7))}\")\n",
);

/// A method parameter of an imported type reads the argument it is given.
#[test]
fn test_a_module_method_can_take_a_type_the_module_imports() {
    assert_project_runs_with_output(
        &[
            ("main.mi", MAIN),
            ("other.mi", OTHER),
            (
                "sig.mi",
                concat!(
                    "use local.other.{Foo}\n",
                    "\n",
                    "public class Q\n",
                    "    public var n int\n",
                    "    public fn take(f Foo) int\n",
                    "        return f.v + self.n\n",
                ),
            ),
        ],
        "9",
    );
}

/// An alias the module declares of a type it imports types a method too.
#[test]
fn test_a_module_method_can_take_an_alias_of_a_type_the_module_imports() {
    assert_project_runs_with_output(
        &[
            ("main.mi", MAIN),
            ("other.mi", OTHER),
            (
                "sig.mi",
                concat!(
                    "use local.other.{Foo}\n",
                    "\n",
                    "public type Box is Foo\n",
                    "\n",
                    "public class Q\n",
                    "    public var n int\n",
                    "    public fn take(f Box) int\n",
                    "        return f.v * self.n\n",
                ),
            ),
        ],
        "14",
    );
}

/// A method returning an imported type hands back a value whose fields the
/// caller reads.
#[test]
fn test_a_module_method_can_return_a_type_the_module_imports() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use local.sig.{Q}\n",
                    "use local.other.{Foo}\n",
                    "\n",
                    "fn main()\n",
                    "    let q = Q(n: 5)\n",
                    "    let f = q.make()\n",
                    "    println(f\"{f.v}\")\n",
                ),
            ),
            ("other.mi", OTHER),
            (
                "sig.mi",
                concat!(
                    "use local.other.{Foo}\n",
                    "\n",
                    "public class Q\n",
                    "    public var n int\n",
                    "    public fn make() Foo\n",
                    "        return Foo(v: self.n + 1)\n",
                ),
            ),
        ],
        "6",
    );
}

/// A type the module never imports is still unknown in its signatures.
#[test]
fn test_a_module_method_cannot_take_a_type_the_module_does_not_import() {
    assert_project_compiler_error(
        &[
            ("main.mi", MAIN),
            ("other.mi", OTHER),
            (
                "sig.mi",
                concat!(
                    "public class Q\n",
                    "    public var n int\n",
                    "    public fn take(f Foo) int\n",
                    "        return f.v\n",
                ),
            ),
        ],
        "Unknown type: Foo",
    );
}
