// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A variant may be named through its enum written with type arguments,
//! `GE<String>.Has(..)` and `GE<int>.Nothing`, as well as through the bare
//! enum.

use super::utils::*;

const GE: &str = "
enum GE<T>
    Has(T)
    Nothing
";

#[test]
fn a_variant_with_a_payload_built_through_written_arguments() {
    assert_heap_guard_output(
        &format!(
            "{GE}
fn main()
    let e = GE<String>.Has(\"z\")
    match e
        GE.Has(v): println(v)
        GE.Nothing: println(\"none\")
"
        ),
        "z",
    );
}

#[test]
fn a_variant_without_a_payload_named_through_written_arguments() {
    assert_runs_with_output(
        &format!(
            "{GE}
fn show(e GE<int>) String
    var s = \"none\"
    match e
        GE.Has(v): s = f\"has {{v}}\"
        GE.Nothing: s = \"none\"
    return s

fn main()
    let a = GE<int>.Nothing
    let b GE<int> = GE<int>.Has(4)
    println(f\"{{show(a)}} {{show(b)}}\")
"
        ),
        "none has 4",
    );
}
