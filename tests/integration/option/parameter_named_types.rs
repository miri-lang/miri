// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A program may name its own types after the type parameters the standard
//! library's generic bodies declare. Inside such a body the name means the
//! parameter, so an optional of it is laid out as the parameter's, never as the
//! program's type of the same name.

use crate::integration::utils::*;

#[test]
fn a_struct_named_like_a_map_value_parameter_compiles_and_runs() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.map

struct V
    x int

fn main()
    let a = V(x: 1)
    var m = Map<String, int>()
    m["k"] = 4
    let got = m.get("k") ?? 0
    println(f"{a.x} {got}")
"#,
        "1 4",
    );
}

#[test]
fn classes_named_like_collection_parameters_compile_and_run() {
    assert_heap_guard_output(
        r#"
use system.io
use system.collections.list
use system.collections.map

class K
    var n int

class T
    var n int

fn main()
    var xs = List<T>()
    xs.push(T(n: 2))
    var m = Map<String, K>()
    m["a"] = K(n: 3)
    var popped = 0
    match xs.pop()
        Some(t): popped = t.n
        None: popped = -1
    match m.get("a")
        Some(k): println(f"{k.n} {popped}")
        None: println("none")
"#,
        "3 2",
    );
}
