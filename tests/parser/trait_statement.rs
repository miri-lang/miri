// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use super::utils::*;
use miri::ast::factory::*;
use miri::ast::*;
use miri::error::syntax::SyntaxErrorKind;

// ===== Basic Trait Declaration =====

#[test]
fn test_simple_trait() {
    // Traits currently require method bodies (abstract methods to be added later)
    let source = "trait Drawable\n    fn draw()\n        x";
    let program = parse_program(source);
    assert_eq!(program.body.len(), 1);
    if let StatementKind::Trait(name, _, _, body, _) = &program.body[0].node {
        assert_eq!(
            name.node,
            ExpressionKind::Identifier("Drawable".into(), None)
        );
        assert_eq!(body.len(), 1);
    } else {
        panic!("Expected trait statement");
    }
}

#[test]
fn test_trait_with_multiple_methods() {
    let source = "trait Comparable\n    fn compare(other int) int\n        0\n    fn equals(other int) bool\n        true";
    let program = parse_program(source);
    if let StatementKind::Trait(_, _, _, body, _) = &program.body[0].node {
        assert_eq!(body.len(), 2);
    } else {
        panic!("Expected trait statement");
    }
}

// ===== Trait Inheritance (Multiple) =====

#[test]
fn test_trait_extends_single() {
    let source = "trait Sortable extends Comparable\n    fn sort()\n        x";
    let program = parse_program(source);
    if let StatementKind::Trait(name, _, parent_traits, body, _) = &program.body[0].node {
        assert_eq!(
            name.node,
            ExpressionKind::Identifier("Sortable".into(), None)
        );
        assert_eq!(parent_traits.len(), 1);
        assert_eq!(body.len(), 1);
    } else {
        panic!("Expected trait statement");
    }
}

#[test]
fn test_trait_extends_multiple() {
    let source = "trait ReadWrite extends Readable, Writable\n    fn readwrite()\n        x";
    let program = parse_program(source);
    if let StatementKind::Trait(_, _, parent_traits, _, _) = &program.body[0].node {
        assert_eq!(parent_traits.len(), 2);
        if let ExpressionKind::Identifier(name1, None) = &parent_traits[0].node {
            assert_eq!(name1, "Readable");
        } else {
            panic!("Expected identifier");
        }
        if let ExpressionKind::Identifier(name2, None) = &parent_traits[1].node {
            assert_eq!(name2, "Writable");
        } else {
            panic!("Expected identifier");
        }
    } else {
        panic!("Expected trait statement");
    }
}

// ===== Trait with Generics =====

#[test]
fn test_trait_with_generics() {
    let source = "trait Container<T>\n    fn add(item T)\n        x";
    let program = parse_program(source);
    if let StatementKind::Trait(_, generics, _, _, _) = &program.body[0].node {
        assert!(generics.is_some());
        assert_eq!(generics.as_ref().unwrap().len(), 1);
    } else {
        panic!("Expected trait statement");
    }
}

#[test]
fn test_trait_with_generic_constraint() {
    let source = "trait OrderedContainer<T extends Comparable>\n    fn sort()\n        x";
    let program = parse_program(source);
    if let StatementKind::Trait(_, generics, _, _, _) = &program.body[0].node {
        assert!(generics.is_some());
        let generic = &generics.as_ref().unwrap()[0];
        if let ExpressionKind::GenericType(_, constraint, _) = &generic.node {
            assert!(constraint.is_some());
        } else {
            panic!("Expected generic type");
        }
    } else {
        panic!("Expected trait statement");
    }
}

// ===== Visibility =====

#[test]
fn test_public_trait() {
    // Top-level traits default to public visibility
    let source = "trait API\n    fn call()\n        x";
    let program = parse_program(source);
    if let StatementKind::Trait(_, _, _, _, vis) = &program.body[0].node {
        assert_eq!(*vis, MemberVisibility::Public);
    } else {
        panic!("Expected trait statement");
    }
}

// ===== Error Cases =====

#[test]
fn test_error_trait_with_implements() {
    // Traits use extends for inheritance, not implements
    parser_error_test(
        "trait Invalid implements Something\n    fn method()\n        x",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "end of expression".to_string(),
            found: "implements".to_string(),
        },
    );
}

#[test]
fn test_error_trait_invalid_member() {
    parser_error_test(
        "trait Invalid\n    for x in y",
        &SyntaxErrorKind::UnexpectedToken {
            expected: "class member (let, var, fn, async, gpu, or type)".to_string(),
            found: "for".to_string(),
        },
    );
}
