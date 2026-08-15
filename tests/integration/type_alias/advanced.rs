// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn type_alias_with_nullable() {
    assert_runs(
        r#"
type OptionalInt is int?
var x OptionalInt = 5
x = None
"#,
    );
}

#[test]
fn type_alias_to_nullable_wraps_a_bare_initializer() {
    // The alias names an optional, so `5` has to be wrapped before it is
    // stored. Reading the value back is what proves the wrap happened: a raw
    // integer left in the slot still assigns, and only fails once something
    // treats it as the optional the declaration promised.
    assert_runs_with_output(
        r#"
use system.io

type OptionalInt is int?
let x OptionalInt = 5
match x
    Some(v): println(f"some {v}")
    None: println("none")
"#,
        "some 5\n",
    );
}

#[test]
fn type_alias_to_tuple_is_released() {
    // The alias stands for a tuple, which the backend allocates. The
    // declaration must not hide that behind the alias name, or the block is
    // never freed.
    assert_runs_with_output(
        r#"
use system.io

type Pair is (int, int)
let p Pair = (1, 2)
println(f"{p.0} {p.1}")
"#,
        "1 2\n",
    );
}

#[test]
fn type_alias_in_struct() {
    assert_runs(
        r#"
type MyInt is int

struct Point
    x MyInt
    y MyInt

let p = Point(1, 2)
"#,
    );
}

#[test]
fn type_alias_in_for_loop() {
    assert_runs(
        r#"
type Numbers is [int; 3]
let nums Numbers = [1, 2, 3]
for n in nums
    let x = n * 2
"#,
    );
}
