// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Top-level functions of one name declared in different modules are distinct
//! definitions: each call runs the body of the declaration its name resolved
//! to where the call is written.

use super::utils::*;

/// Two modules each keep a private `helper`; each module's public function
/// calls its own.
#[test]
fn test_private_helpers_of_two_modules_keep_their_own_bodies() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.parts.one.{first}\n",
                    "use local.parts.two.{second}\n",
                    "\n",
                    "fn main()\n",
                    "    println(f\"{first()} {second()}\")\n",
                ),
            ),
            (
                "parts/one.mi",
                concat!(
                    "fn helper() int\n",
                    "    return 1\n",
                    "\n",
                    "public fn first() int\n",
                    "    return helper()\n",
                ),
            ),
            (
                "parts/two.mi",
                concat!(
                    "fn helper() int\n",
                    "    return 2\n",
                    "\n",
                    "public fn second() int\n",
                    "    return helper()\n",
                ),
            ),
        ],
        "1 2",
    );
}

/// The program's own `helper` replaces neither module's private one, and a
/// call in the program still runs the program's.
#[test]
fn test_program_function_leaves_module_private_helpers_their_bodies() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.m.a.{from_a}\n",
                    "use local.m.b.{from_b}\n",
                    "\n",
                    "fn helper() int\n",
                    "    return 3\n",
                    "\n",
                    "fn main()\n",
                    "    println(f\"{from_a()} {from_b()} {helper()}\")\n",
                ),
            ),
            (
                "m/a.mi",
                concat!(
                    "private fn helper() int\n",
                    "    return 1\n",
                    "\n",
                    "public fn from_a() int\n",
                    "    return helper()\n",
                ),
            ),
            (
                "m/b.mi",
                concat!(
                    "private fn helper() int\n",
                    "    return 2\n",
                    "\n",
                    "public fn from_b() int\n",
                    "    return helper()\n",
                ),
            ),
        ],
        "1 2 3",
    );
}

/// A program function spelled like a private helper of the standard library
/// leaves the library's function computing what it did, and is itself
/// callable.
#[test]
fn test_program_function_named_like_a_stdlib_private_helper_leaves_the_stdlib_result() {
    assert_runs_with_output(
        concat!(
            "use system.io\n",
            "use system.math.{value_noise}\n",
            "\n",
            "fn lattice_unit(_a float, _b float) float\n",
            "    return 1000.0\n",
            "\n",
            "fn main()\n",
            "    println(f\"{value_noise(0.3, 0.7)} {lattice_unit(0.0, 0.0)}\")\n",
        ),
        "0.26472268779495794 1000.0",
    );
}

/// A generic function declared in two modules under one name compiles one
/// body per declaration at each instantiation.
#[test]
fn test_generic_functions_of_two_modules_keep_their_own_bodies() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.parts.one.{first}\n",
                    "use local.parts.two.{second}\n",
                    "\n",
                    "fn main()\n",
                    "    println(f\"{first()} {second()}\")\n",
                ),
            ),
            (
                "parts/one.mi",
                concat!(
                    "fn tag<T>(_x T) int\n",
                    "    return 10\n",
                    "\n",
                    "public fn first() int\n",
                    "    return tag(5)\n",
                ),
            ),
            (
                "parts/two.mi",
                concat!(
                    "fn tag<T>(_x T) int\n",
                    "    return 20\n",
                    "\n",
                    "public fn second() int\n",
                    "    return tag(5)\n",
                ),
            ),
        ],
        "10 20",
    );
}

/// A private helper passed as a function value inside its module forwards to
/// that module's body, not to another module's function of the same name.
#[test]
fn test_function_reference_to_a_module_helper_forwards_to_its_own_body() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.io\n",
                    "use local.parts.one.{first}\n",
                    "\n",
                    "fn helper(x int) int\n",
                    "    return x + 300\n",
                    "\n",
                    "fn main()\n",
                    "    println(f\"{first()} {helper(0)}\")\n",
                ),
            ),
            (
                "parts/one.mi",
                concat!(
                    "fn helper(x int) int\n",
                    "    return x + 100\n",
                    "\n",
                    "fn apply(f fn(x int) int, x int) int\n",
                    "    return f(x)\n",
                    "\n",
                    "public fn first() int\n",
                    "    return apply(helper, 1)\n",
                ),
            ),
        ],
        "101 300",
    );
}

/// Two modules' helpers of one name, and the program's own, each run their
/// own body when one kernel reaches all three.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn test_same_named_helpers_of_two_modules_reached_from_gpu_code_keep_their_bodies() {
    assert_project_runs_with_output(
        &[
            (
                "main.mi",
                concat!(
                    "use system.gpu\n",
                    "use system.collections.array\n",
                    "use local.k.a.{from_a}\n",
                    "use local.k.b.{from_b}\n",
                    "\n",
                    "fn helper(x int) int\n",
                    "    return x * 10000\n",
                    "\n",
                    "fn main()\n",
                    "    gpu let src = [1, 2, 3]\n",
                    "    gpu var dst = [0, 0, 0]\n",
                    "    gpu forall i in 0..3\n",
                    "        dst[i] = helper(src[i]) + from_a(src[i]) * 1000 + from_b(src[i])\n",
                    "    let host = dst\n",
                    "    println(f'{host[0]} {host[1]} {host[2]}')\n",
                ),
            ),
            (
                "k/a.mi",
                concat!(
                    "fn helper(x int) int\n",
                    "    return x + 1\n",
                    "\n",
                    "public fn from_a(x int) int\n",
                    "    return helper(x)\n",
                ),
            ),
            (
                "k/b.mi",
                concat!(
                    "fn helper(x int) int\n",
                    "    return x + 100\n",
                    "\n",
                    "public fn from_b(x int) int\n",
                    "    return helper(x)\n",
                ),
            ),
        ],
        "12101 23102 34103",
    );
}

/// A module a `use` names `Main` is a module like any other: its private
/// `helper` is not the program's own `helper`, whatever the program's file is
/// called internally.
#[test]
fn test_module_named_main_keeps_its_private_helper() {
    assert_project_runs_with_output(
        &[
            (
                "app.mi",
                concat!(
                    "use system.io\n",
                    "use Main.{main2}\n",
                    "\n",
                    "fn helper() int\n",
                    "    return 1\n",
                    "\n",
                    "fn main()\n",
                    "    println(f\"{main2()} {helper()}\")\n",
                ),
            ),
            (
                "Main.mi",
                concat!(
                    "fn helper() int\n",
                    "    return 99\n",
                    "\n",
                    "public fn main2() int\n",
                    "    return helper()\n",
                ),
            ),
        ],
        "99 1",
    );
}
