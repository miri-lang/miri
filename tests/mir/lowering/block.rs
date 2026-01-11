// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::super::utils::{
    assert_locals, assert_terminator, count_assignments, count_assignments_to, find_local_idx,
    get_assignment_order, has_local, lower_code,
};
use miri::mir::TerminatorKind;

#[test]
fn test_many_statements_preserve_order() {
    let source = "
fn main()
    let v1 = 1
    let v2 = 2
    let v3 = 3
    let v4 = 4
    let v5 = 5
";
    let body = lower_code(source);

    let indices: Vec<_> = (1..=5)
        .map(|i| find_local_idx(&body, &format!("v{i}")).expect(&format!("v{i} not found")))
        .collect();

    let order = get_assignment_order(&body, 0);
    let positions: Vec<_> = indices
        .iter()
        .map(|&idx| order.iter().position(|&x| x == idx).unwrap())
        .collect();

    for i in 0..positions.len() - 1 {
        assert!(
            positions[i] < positions[i + 1],
            "v{} should come before v{}",
            i + 1,
            i + 2
        );
    }
}

#[test]
fn test_linear_flow_single_basic_block() {
    let source = "
fn main()
    let x = 10
    let y = 20
    let z = 30
";
    let body = lower_code(source);

    assert_eq!(
        body.basic_blocks.len(),
        1,
        "Sequential statements should produce exactly 1 basic block"
    );
}

#[test]
fn test_empty_body_single_basic_block() {
    let source = "fn main(): 0";
    let body = lower_code(source);

    assert_eq!(
        body.basic_blocks.len(),
        1,
        "Empty body should have 1 basic block"
    );
}

#[test]
fn test_implicit_return_terminator() {
    assert_terminator(
        "
fn main()
    let x = 42
",
        TerminatorKind::Return,
    );
}

#[test]
fn test_explicit_return_terminator() {
    assert_terminator("fn main(): return\n", TerminatorKind::Return);
}

#[test]
fn test_return_after_statements() {
    assert_terminator(
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
    let body = lower_code(source);

    assert!(has_local(&body, "x"), "x should exist");
    assert!(count_assignments(&body, 0) >= 1, "Should have assignment");
}

#[test]
fn test_inline_block_expression() {
    let source = "fn main(): 42";
    let body = lower_code(source);

    assert!(body.local_decls.len() >= 1, "Should have return local");
    assert!(
        count_assignments(&body, 0) >= 1,
        "Should have expression assignment"
    );
}

#[test]
fn test_inline_block_binary_expression() {
    let source = "fn main(): 1 + 2";
    let body = lower_code(source);

    // Binary expression creates a temp for result
    assert!(
        count_assignments(&body, 0) >= 1,
        "Should have binary op assignment"
    );
}

#[test]
fn test_single_variable_declaration() {
    let source = "fn main(): let x = 42";
    let body = lower_code(source);

    assert!(has_local(&body, "x"), "x should exist");
    let x_idx = find_local_idx(&body, "x").unwrap();
    assert_eq!(
        count_assignments_to(&body, 0, x_idx),
        1,
        "x should have 1 assignment"
    );
}

#[test]
fn test_multiple_variable_declarations() {
    assert_locals(
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
    assert_locals(
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
    let source = "
fn main()
    var x = 1
    x = 2
";
    let body = lower_code(source);

    let x_idx = find_local_idx(&body, "x").expect("x not found");
    assert_eq!(
        count_assignments_to(&body, 0, x_idx),
        2,
        "x should have initial + 1 reassignment"
    );
}

#[test]
fn test_multiple_reassignments() {
    let source = "
fn main()
    var x = 1
    x = 2
    x = 3
    x = 4
";
    let body = lower_code(source);

    let x_idx = find_local_idx(&body, "x").expect("x not found");
    assert_eq!(
        count_assignments_to(&body, 0, x_idx),
        4,
        "x should have initial + 3 reassignments"
    );
}

#[test]
fn test_reassignment_with_expression() {
    let source = "
fn main()
    var x = 1
    x = x + 1
";
    let body = lower_code(source);

    let x_idx = find_local_idx(&body, "x").expect("x not found");
    assert_eq!(
        count_assignments_to(&body, 0, x_idx),
        2,
        "x should have 2 assignments"
    );
}

#[test]
fn test_chained_expressions() {
    let source = "
fn main()
    let a = 1
    let b = a + 2
    let c = b * 3
";
    let body = lower_code(source);

    let idx_a = find_local_idx(&body, "a").unwrap();
    let idx_b = find_local_idx(&body, "b").unwrap();
    let idx_c = find_local_idx(&body, "c").unwrap();

    let order = get_assignment_order(&body, 0);
    let pos_a = order.iter().position(|&x| x == idx_a).unwrap();
    let pos_b = order.iter().position(|&x| x == idx_b).unwrap();
    let pos_c = order.iter().position(|&x| x == idx_c).unwrap();

    assert!(
        pos_a < pos_b && pos_b < pos_c,
        "Chained expressions should maintain order"
    );
}

#[test]
fn test_nested_binary_expressions() {
    let source = "fn main(): (1 + 2) * (3 + 4)";
    let body = lower_code(source);

    // Should create temps for sub-expressions
    assert!(
        count_assignments(&body, 0) >= 3,
        "Should have multiple assignments for nested ops"
    );
}

#[test]
fn test_minimal_function() {
    let source = "fn main(): 0";
    let body = lower_code(source);

    assert_eq!(body.basic_blocks.len(), 1);
    assert!(body.basic_blocks[0].terminator.is_some());
}

#[test]
fn test_function_with_only_return() {
    let source = "
fn main()
    return
";
    let body = lower_code(source);

    let term = body.basic_blocks[0]
        .terminator
        .as_ref()
        .expect("No terminator");
    assert!(matches!(term.kind, TerminatorKind::Return));
}

#[test]
fn test_variable_reference_in_expression() {
    let source = "
fn main()
    let x = 10
    let y = x
";
    let body = lower_code(source);

    assert!(has_local(&body, "x"), "x should exist");
    assert!(has_local(&body, "y"), "y should exist");

    // y's initializer uses x, which should create a Copy operand
    assert!(count_assignments(&body, 0) >= 2);
}

#[test]
fn test_unary_expression_in_block() {
    assert_locals(
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
    assert_locals(
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
    assert_locals(
        "
fn main()
    let x = 5
    let y = 10
    let cmp = x < y
",
        &["x", "y", "cmp"],
    );
}
