// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A function whose locals outgrow a guard page recurses and returns the
//! right value, so the stack probes its prologue carries leave the frame
//! intact. One frame takes a single probe, the other a probe loop.

use super::utils::*;

/// A recursive `f` holding `locals` 128-byte structs, called as `f(3)`, which
/// sums the second field of every one of them on each of its four levels.
fn recursion_through_frame_of(locals: usize) -> String {
    let fields: String = (0..16).map(|i| format!("    a{i} int\n")).collect();
    let bindings: String = (0..locals)
        .map(|i| {
            format!("    let b{i} = Big(n, {i}, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15)\n")
        })
        .collect();
    let sum: Vec<String> = (0..locals).map(|i| format!("b{i}.a1")).collect();
    let sum = sum.join(" + ");
    format!(
        "use system.io\n\nstruct Big\n{fields}\nfn f(n int) int\n{bindings}    let here = {sum}\n    \
         if n == 0\n        return here\n    return f(n - 1) + here\n\nfn main()\n    println(f\"{{f(3)}}\")\n"
    )
}

/// What `f(3)` returns for `locals` structs: four levels of 0 + 1 + … + (locals − 1).
fn expected_sum(locals: usize) -> String {
    (4 * locals * (locals - 1) / 2).to_string()
}

#[test]
fn a_frame_one_guard_page_deep_recurses_intact() {
    let locals = 40;
    assert_runs_with_output(&recursion_through_frame_of(locals), &expected_sum(locals));
}

#[test]
fn a_frame_several_guard_pages_deep_recurses_intact() {
    let locals = 200;
    assert_runs_with_output(&recursion_through_frame_of(locals), &expected_sum(locals));
}
