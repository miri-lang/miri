// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::{assert_runs, assert_runs_many, assert_runs_with_output};

/// A deeply nested but valid expression must compile and run, not overflow the
/// compiler's native stack. A long left-associative chain builds a deep
/// left-leaning AST that type inference and MIR lowering walk recursively; the
/// compiler runs those passes on a large worker stack so depth is bounded by
/// memory rather than a fixed stack.
#[test]
fn deep_binary_chain_compiles_and_runs() {
    // ~5000 additions: comfortably past the ~3500-deep point that previously
    // aborted the compiler with a stack overflow.
    let chain: String = " + 1".repeat(5000);
    assert_runs_with_output(&format!("println(f'{{1{}}}')", chain), "5001");
}

/// A shadowing `let` whose initializer references the shadowed name must read
/// the *outer* binding, not the new one being declared (which is not yet in
/// scope during its own initializer). `let x = x + 100` after `let x = 5` must
/// evaluate to `105`, not `100`.
#[test]
fn shadowing_let_initializer_reads_outer_binding() {
    assert_runs_with_output("let x = 5\nlet x = x + 100\nprintln(f'{x}')", "105");
}

/// The same rule inside a loop body: shadowing the loop variable with an
/// initializer that uses it must read the loop value each iteration.
#[test]
fn shadowing_loop_variable_reads_loop_value() {
    assert_runs_with_output(
        "for i in 1..4:\n  let i = i + 100\n  println(f'{i}')",
        "101\n102\n103",
    );
}

#[test]
fn empty_program() {
    assert_runs("");
}

#[test]
fn program_with_integer_literal() {
    assert_runs("42");
}

#[test]
fn program_with_multiple_literals() {
    assert_runs(
        r#"
123.456
"Hello, World!"
1000
"#,
    );
}

#[test]
fn comments_only() {
    assert_runs("// just a comment");
    assert_runs(
        r#"
// comment 1
// comment 2
"#,
    );
}

#[test]
fn whitespace_only() {
    assert_runs_many(&["   ", "\n\n", "  \n  \t  "]);
}

#[test]
fn basic_literals() {
    assert_runs_many(&["true", "false", "1.23", r#""string""#]);
}

#[test]
fn integer_edge_cases() {
    assert_runs_many(&["0", "-1", "9223372036854775807", "-9223372036854775808"]);
}

#[test]
fn mixed_basic_program() {
    assert_runs(
        r#"
    // Start
    123
    "text"
    // End
"#,
    );
}

/// Multiline expression inside parentheses: a newline between `(` and `)`
/// must not terminate the statement, even when the parens appear inside an
/// indented block.
#[test]
fn multiline_parenthesized_expression_inside_function_body() {
    assert_runs_with_output(
        r#"
fn main()
    let x = (1 +
        2 +
        3)
    println(f"{x}")
"#,
        "6",
    );
}

/// Multiline list literal inside an indented block.
#[test]
fn multiline_list_literal_inside_function_body() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let xs = [
        1,
        2,
        3,
        4
    ]
    println(f"{xs[0]}, {xs[3]}")
"#,
        "1, 4",
    );
}

/// Multiline map literal inside an indented block.
#[test]
fn multiline_map_literal_inside_function_body() {
    assert_runs_with_output(
        r#"
use system.collections.map

fn main()
    let m = {
        'a': 1,
        'b': 2,
        'c': 3
    }
    println(f"{m['b']}")
"#,
        "2",
    );
}

/// Multiline set literal inside an indented block.
#[test]
fn multiline_set_literal_inside_function_body() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    let s = {
        1,
        2,
        3
    }
    println(f"{s.length()}")
"#,
        "3",
    );
}

/// Multiline function-call argument list inside an indented block.
#[test]
fn multiline_function_call_inside_function_body() {
    assert_runs_with_output(
        r#"

fn add(a int, b int, c int) int
    a + b + c

fn main()
    let x = add(
        1,
        2,
        3
    )
    println(f"{x}")
"#,
        "6",
    );
}

/// Deeply nested multiline brackets: list of lists inside a function body.
#[test]
fn multiline_nested_collections_inside_function_body() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let m = [
        [
            1,
            2
        ],
        [3, 4]
    ]
    println(f"{m[0][1]}, {m[1][0]}")
"#,
        "2, 3",
    );
}

/// Multiline operator-broken expression continues across newlines while inside
/// parentheses, even when the binary operator sits at the start of the
/// continuation line.
#[test]
fn multiline_binary_op_at_line_start_inside_parens() {
    assert_runs_with_output(
        r#"

fn main()
    let x = (10
        + 20
        + 30)
    println(f"{x}")
"#,
        "60",
    );
}
