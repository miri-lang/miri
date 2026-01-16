// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{
    mir_lowering_assignment_count_test, mir_lowering_basic_blocks_test,
    mir_lowering_has_terminator_test, mir_lowering_local_test, mir_lowering_locals_test,
    mir_lowering_min_assignments_test, mir_lowering_min_locals_test,
    mir_lowering_order_preserved_test, mir_lowering_terminator_test,
};
use miri::mir::TerminatorKind;

#[test]
fn test_many_statements_preserve_order() {
    mir_lowering_order_preserved_test(
        "
fn main()
    let v1 = 1
    let v2 = 2
    let v3 = 3
    let v4 = 4
    let v5 = 5
",
        &["v1", "v2", "v3", "v4", "v5"],
    );
}

#[test]
fn test_linear_flow_single_basic_block() {
    mir_lowering_basic_blocks_test(
        "
fn main()
    let x = 10
    let y = 20
    let z = 30
",
        1,
    );
}

#[test]
fn test_empty_body_single_basic_block() {
    mir_lowering_basic_blocks_test("fn main(): 0", 1);
}

#[test]
fn test_implicit_return_terminator() {
    mir_lowering_terminator_test(
        "
fn main()
    let x = 42
",
        TerminatorKind::Return,
    );
}

#[test]
fn test_explicit_return_terminator() {
    mir_lowering_terminator_test("fn main(): return\n", TerminatorKind::Return);
}

#[test]
fn test_return_after_statements() {
    mir_lowering_terminator_test(
        "
fn main()
    let x = 1
    let y = 2
    return
",
        TerminatorKind::Return,
    );
}

#[test]
fn test_inline_block_single_statement() {
    let source = "fn main(): let x = 10";
    mir_lowering_local_test(source, "x");
    mir_lowering_min_assignments_test(source, 0, 1);
}

#[test]
fn test_inline_block_expression() {
    let source = "fn main(): 42";
    mir_lowering_min_locals_test(source, 1);
    mir_lowering_min_assignments_test(source, 0, 1);
}

#[test]
fn test_inline_block_binary_expression() {
    mir_lowering_min_assignments_test("fn main(): 1 + 2", 0, 1);
}

#[test]
fn test_single_variable_declaration() {
    let source = "fn main(): let x = 42";
    mir_lowering_local_test(source, "x");
    mir_lowering_assignment_count_test(source, "x", 1);
}

#[test]
fn test_multiple_variable_declarations() {
    mir_lowering_locals_test(
        "
fn main()
    let a = 1
    let b = 2
    let c = 3
",
        &["a", "b", "c"],
    );
}

#[test]
fn test_variable_with_expression_initializer() {
    mir_lowering_locals_test(
        "
fn main()
    let x = 5
    let y = x + 1
",
        &["x", "y"],
    );
}

#[test]
fn test_single_reassignment() {
    mir_lowering_assignment_count_test(
        "
fn main()
    var x = 1
    x = 2
",
        "x",
        2,
    );
}

#[test]
fn test_multiple_reassignments() {
    mir_lowering_assignment_count_test(
        "
fn main()
    var x = 1
    x = 2
    x = 3
    x = 4
",
        "x",
        4,
    );
}

#[test]
fn test_reassignment_with_expression() {
    mir_lowering_assignment_count_test(
        "
fn main()
    var x = 1
    x = x + 1
",
        "x",
        2,
    );
}

#[test]
fn test_chained_expressions() {
    mir_lowering_order_preserved_test(
        "
fn main()
    let a = 1
    let b = a + 2
    let c = b * 3
",
        &["a", "b", "c"],
    );
}

#[test]
fn test_nested_binary_expressions() {
    mir_lowering_min_assignments_test("fn main(): (1 + 2) * (3 + 4)", 0, 3);
}

#[test]
fn test_minimal_function() {
    let source = "fn main(): 0";
    mir_lowering_basic_blocks_test(source, 1);
    mir_lowering_has_terminator_test(source, 0);
}

#[test]
fn test_function_with_only_return() {
    mir_lowering_terminator_test(
        "
fn main()
    return
",
        TerminatorKind::Return,
    );
}

#[test]
fn test_variable_reference_in_expression() {
    let source = "
fn main()
    let x = 10
    let y = x
";
    mir_lowering_locals_test(source, &["x", "y"]);
    mir_lowering_min_assignments_test(source, 0, 2);
}

#[test]
fn test_unary_expression_in_block() {
    mir_lowering_locals_test(
        "
fn main()
    let x = 5
    let y = -x
",
        &["x", "y"],
    );
}

#[test]
fn test_boolean_expressions() {
    mir_lowering_locals_test(
        "
fn main()
    let a = true
    let b = false
    let c = not a
",
        &["a", "b", "c"],
    );
}

#[test]
fn test_comparison_expression() {
    mir_lowering_locals_test(
        "
fn main()
    let x = 5
    let y = 10
    let cmp = x < y
",
        &["x", "y", "cmp"],
    );
}

#[test]
fn test_many_variables_preserve_all() {
    mir_lowering_locals_test(
        "
fn main()
    let a = 1
    let b = 2
    let c = 3
    let d = 4
    let e = 5
    let f = 6
    let g = 7
    let h = 8
    let i = 9
    let j = 10
",
        &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"],
    );
}

#[test]
fn test_ten_reassignments() {
    mir_lowering_assignment_count_test(
        "
fn main()
    var x = 0
    x = 1
    x = 2
    x = 3
    x = 4
    x = 5
    x = 6
    x = 7
    x = 8
    x = 9
",
        "x",
        10,
    );
}

#[test]
fn test_deeply_nested_expression() {
    mir_lowering_min_assignments_test("fn main(): ((((((1 + 2) * 3) - 4) / 5) % 6) + 7)", 0, 1);
}

#[test]
fn test_inline_with_variable_reference() {
    let source = "fn main(): let x = 1 + 2 * 3 - 4";
    mir_lowering_local_test(source, "x");
    mir_lowering_min_assignments_test(source, 0, 1);
}

#[test]
fn test_expression_only_block() {
    mir_lowering_min_assignments_test("fn main(): 1 + 2", 0, 1);
}

#[test]
fn test_return_value_direct() {
    mir_lowering_terminator_test("fn main() int: 42", TerminatorKind::Return);
}

#[test]
fn test_return_string_literal() {
    mir_lowering_terminator_test("fn main() string: \"hello\"", TerminatorKind::Return);
}

#[test]
fn test_return_boolean() {
    mir_lowering_terminator_test("fn main() bool: true", TerminatorKind::Return);
}

#[test]
fn test_many_temp_variables_from_expression() {
    mir_lowering_min_assignments_test("fn main(): 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10", 0, 1);
}
