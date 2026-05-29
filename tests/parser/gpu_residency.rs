// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// Parser surface for the residency keywords `gpu let` and `gpu var`.

use super::utils::parse_program;
use miri::ast::statement::{BindingResidency, StatementKind, VariableDeclarationType};

fn declarations(input: &str) -> Vec<miri::ast::statement::VariableDeclaration> {
    let program = parse_program(input);
    assert_eq!(program.body.len(), 1);
    match &program.body[0].node {
        StatementKind::Variable(decls, _) => decls.clone(),
        other => panic!("Expected variable declaration, got {:?}", other),
    }
}

#[test]
fn test_gpu_let_parses_with_gpu_residency() {
    let decls = declarations("gpu let g = 0");
    assert_eq!(decls.len(), 1);
    assert_eq!(decls[0].name, "g");
    assert_eq!(
        decls[0].declaration_type,
        VariableDeclarationType::Immutable
    );
    assert_eq!(decls[0].residency, BindingResidency::Gpu);
}

#[test]
fn test_gpu_var_parses_with_gpu_residency_and_is_mutable() {
    let decls = declarations("gpu var g = 0");
    assert_eq!(decls[0].declaration_type, VariableDeclarationType::Mutable);
    assert_eq!(decls[0].residency, BindingResidency::Gpu);
}

#[test]
fn test_plain_let_defaults_to_host_residency() {
    let decls = declarations("let h = 0");
    assert_eq!(decls[0].residency, BindingResidency::Host);
}

#[test]
fn test_plain_var_defaults_to_host_residency() {
    let decls = declarations("var h = 0");
    assert_eq!(decls[0].residency, BindingResidency::Host);
}

#[test]
fn test_gpu_const_is_rejected() {
    use miri::error::syntax::SyntaxErrorKind;
    super::utils::parser_error_test(
        "gpu const x = 0",
        &SyntaxErrorKind::InvalidModifierCombination {
            combination: "gpu const".to_string(),
            reason: "Residency on a compile-time constant has no meaning.".to_string(),
        },
    );
}
