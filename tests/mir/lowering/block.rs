// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::{
    count_assignments, count_assignments_to, find_local_idx, get_assignment_order, has_local,
    last_terminator, mir_lower_code, terminator_of,
};
use miri::mir::TerminatorKind;

fn assert_locals(source: &str, expected: &[&str]) {
    let body = mir_lower_code(source);
    for name in expected {
        assert!(
            has_local(&body, name),
            "Expected local '{}' in MIR for source:\n{}",
            name,
            source
        );
    }
}

fn assert_assignments_to(source: &str, var_name: &str, expected_count: usize) {
    let body = mir_lower_code(source);
    let idx = find_local_idx(&body, var_name)
        .unwrap_or_else(|| panic!("Expected local '{}' for source:\n{}", var_name, source));
    assert_eq!(
        count_assignments_to(&body, 0, idx),
        expected_count,
        "Expected {} assignments to '{}' for source:\n{}",
        expected_count,
        var_name,
        source
    );
}

fn assert_at_least_assignments(source: &str, min_count: usize) {
    let body = mir_lower_code(source);
    let actual = count_assignments(&body, 0);
    assert!(
        actual >= min_count,
        "Expected at least {} assignments in block 0, got {} for source:\n{}",
        min_count,
        actual,
        source
    );
}

/// Asserts that the named variables are first assigned in the order given.
fn assert_declaration_order(source: &str, var_names: &[&str]) {
    let body = mir_lower_code(source);
    let order = get_assignment_order(&body, 0);
    let positions: Vec<_> = var_names
        .iter()
        .map(|name| {
            let idx = find_local_idx(&body, name)
                .unwrap_or_else(|| panic!("{} not found in source:\n{}", name, source));
            order
                .iter()
                .position(|&assigned| assigned == idx)
                .unwrap_or_else(|| panic!("{} never assigned in source:\n{}", name, source))
        })
        .collect();

    for (i, window) in positions.windows(2).enumerate() {
        assert!(
            window[0] < window[1],
            "{} should come before {}",
            var_names[i],
            var_names[i + 1]
        );
    }
}

#[test]
fn test_many_statements_preserve_order() {
    assert_declaration_order(
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
    let body = mir_lower_code(
        "
fn main()
    let x = 10
    let y = 20
    let z = 30
",
    );
    assert_eq!(body.basic_blocks.len(), 1);
}

#[test]
fn test_empty_body_single_basic_block() {
    let body = mir_lower_code("fn main(): 0");
    assert_eq!(body.basic_blocks.len(), 1);
}

#[test]
fn test_implicit_return_terminator() {
    let body = mir_lower_code(
        "
fn main()
    let x = 42
",
    );
    assert!(matches!(
        last_terminator(&body).kind,
        TerminatorKind::Return
    ));
}

#[test]
fn test_explicit_return_terminator() {
    let body = mir_lower_code("fn main(): return\n");
    assert!(matches!(
        last_terminator(&body).kind,
        TerminatorKind::Return
    ));
}

#[test]
fn test_return_after_statements() {
    let body = mir_lower_code(
        "
fn main()
    let x = 1
    let y = 2
    return
",
    );
    assert!(matches!(
        last_terminator(&body).kind,
        TerminatorKind::Return
    ));
}

#[test]
fn test_inline_block_single_statement() {
    let source = "fn main(): let x = 10";
    assert_locals(source, &["x"]);
    assert_at_least_assignments(source, 1);
}

#[test]
fn test_inline_block_expression() {
    let source = "fn main(): 42";
    let body = mir_lower_code(source);
    assert!(!body.local_decls.is_empty());
    assert_at_least_assignments(source, 1);
}

#[test]
fn test_inline_block_binary_expression() {
    assert_at_least_assignments("fn main(): 1 + 2", 1);
}

#[test]
fn test_single_variable_declaration() {
    let source = "fn main(): let x = 42";
    assert_locals(source, &["x"]);
    assert_assignments_to(source, "x", 1);
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
    assert_assignments_to(
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
    assert_assignments_to(
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
    assert_assignments_to(
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
    assert_declaration_order(
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
    assert_at_least_assignments("fn main(): (1 + 2) * (3 + 4)", 3);
}

#[test]
fn test_minimal_function() {
    let body = mir_lower_code("fn main(): 0");
    assert_eq!(body.basic_blocks.len(), 1);
    assert!(terminator_of(&body, 0).is_some());
}

#[test]
fn test_function_with_only_return() {
    let body = mir_lower_code(
        "
fn main()
    return
",
    );
    assert!(matches!(
        last_terminator(&body).kind,
        TerminatorKind::Return
    ));
}

#[test]
fn test_variable_reference_in_expression() {
    let source = "
fn main()
    let x = 10
    let y = x
";
    assert_locals(source, &["x", "y"]);
    assert_at_least_assignments(source, 2);
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

#[test]
fn test_many_variables_preserve_all() {
    assert_locals(
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
    assert_assignments_to(
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
    assert_at_least_assignments("fn main(): ((((((1 + 2) * 3) - 4) / 5) % 6) + 7)", 1);
}

#[test]
fn test_inline_with_variable_reference() {
    let source = "fn main(): let x = 1 + 2 * 3 - 4";
    assert_locals(source, &["x"]);
    assert_at_least_assignments(source, 1);
}

#[test]
fn test_expression_only_block() {
    assert_at_least_assignments("fn main(): 1 + 2", 1);
}

#[test]
fn test_return_value_direct() {
    let body = mir_lower_code("fn main() int: 42");
    assert!(matches!(
        last_terminator(&body).kind,
        TerminatorKind::Return
    ));
}

#[test]
fn test_return_string_literal() {
    let body = mir_lower_code("fn main() String: \"hello\"");
    assert!(matches!(
        last_terminator(&body).kind,
        TerminatorKind::Return
    ));
}

#[test]
fn test_return_boolean() {
    let body = mir_lower_code("fn main() bool: true");
    assert!(matches!(
        last_terminator(&body).kind,
        TerminatorKind::Return
    ));
}

#[test]
fn test_many_temp_variables_from_expression() {
    assert_at_least_assignments("fn main(): 1 + 2 + 3 + 4 + 5 + 6 + 7 + 8 + 9 + 10", 1);
}
