// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;
use crate::utils::{miri_build, miri_check, miri_run};

/// Entry-point shapes `check` and `build` must judge alike.
///
/// `check` is what an author runs to decide a file is finished, so a file it
/// accepts and `build` refuses — or the reverse — is a defect whatever the
/// file says. Each shape here is one the two paths once disagreed on, or one a
/// future divergence in how the entry point is prepared would reach first.
const ENTRY_POINT_SHAPES: &[(&str, &str)] = &[
    (
        "bare return under an if in main",
        "fn main()\n    if 1 == 1\n        return\n    println(\"after\")\n",
    ),
    (
        "bare return inside a loop in main",
        "fn main()\n    for i in 0..3\n        if i == 1\n            return\n        println(f\"{i}\")\n",
    ),
    (
        "bare return in a nested block in main",
        "fn main()\n    var n = 2\n    while n > 0\n        n = n - 1\n        if n == 0\n            if true\n                return\n",
    ),
    (
        "bare return in a match arm in main",
        "fn main()\n    let x int? = None\n    match x\n        Some(v)\n            println(f\"{v}\")\n        None\n            return\n",
    ),
    (
        "bare return in a match initialising a binding in main",
        "fn main()\n    let x int? = None\n    let n = match x\n        Some(v): v\n        None\n            return\n    println(f\"{n}\")\n",
    ),
    (
        "main whose last statement is an expression",
        "fn main()\n    println(\"x\")\n    1 + 1\n",
    ),
    (
        "valued return in an undeclared main",
        "fn main()\n    println(\"x\")\n    return 3\n",
    ),
    (
        "bare return in a main declared int",
        "fn main() int\n    if true\n        return\n    return 0\n",
    ),
    (
        "bare return in a function nested in main",
        "fn main()\n    fn helper(n int)\n        if n == 0\n            return\n        println(\"helper\")\n    helper(0)\n    helper(1)\n",
    ),
    (
        "bare return in a script without main",
        "println(\"a\")\nif true\n    return\nprintln(\"b\")\n",
    ),
    (
        "a script reading a list after passing it to a function",
        "use system.collections.list\n\nfn process(x [int])\n    return\n\nlet x = List([1, 2, 3])\nprocess(x)\nprintln(f\"{x.length()}\")\n",
    ),
    (
        "a script passing a clone and reading the original",
        "use system.collections.list\n\nfn process(x [int])\n    return\n\nlet x = List([1, 2, 3])\nprocess(x.clone())\nprintln(f\"{x.length()}\")\n",
    ),
];

#[test]
fn test_check_and_build_agree_on_every_entry_point_shape() {
    let disagreements: Vec<String> = ENTRY_POINT_SHAPES
        .iter()
        .filter_map(|(shape, source)| {
            let check = miri_check(source);
            let build = miri_build(source);
            (check.success != build.success).then(|| {
                format!(
                    "{shape}: check {} but build {}\n--- check ---\n{}\n--- build ---\n{}",
                    verdict(check.success),
                    verdict(build.success),
                    check.output(),
                    build.output()
                )
            })
        })
        .collect();

    assert!(
        disagreements.is_empty(),
        "check and build disagree:\n{}",
        disagreements.join("\n\n")
    );
}

fn verdict(success: bool) -> &'static str {
    if success {
        "accepts"
    } else {
        "refuses"
    }
}

#[test]
fn test_bare_return_in_main_ends_the_program_successfully() {
    assert_runs_with_output(
        r#"
fn helper(n int)
    if n == 0
        return
    println("helper ran")

fn main()
    helper(0)
    println("before")
    if 1 == 1
        return
    println("unreachable")
"#,
        "before",
    );
}

#[test]
fn test_bare_return_from_a_loop_in_main_stops_the_loop() {
    assert_runs_with_output(
        r#"
fn main()
    for i in 0..5
        if i == 2
            return
        println(f"{i}")
"#,
        "0\n1",
    );
}

#[test]
fn test_bare_return_in_a_match_binding_in_main_exits_before_the_use() {
    assert_runs_with_output(
        r#"
fn main()
    let x int? = None
    println("start")
    let n = match x
        Some(v): v
        None
            return
    println(f"{n}")
"#,
        "start",
    );
}

#[test]
fn test_valued_return_in_an_undeclared_main_is_the_exit_status() {
    let result = miri_run("fn main()\n    println(\"x\")\n    return 3\n");
    assert!(
        !result.success && result.stdout.contains('x'),
        "`return 3` from main should print and then exit with status 3:\n{}",
        result.output()
    );
}

#[test]
fn test_bare_return_in_a_main_declared_int_is_refused() {
    assert_compiler_error(
        "fn main() int\n    if true\n        return\n    return 0\n",
        "expected int, got void",
    );
}
