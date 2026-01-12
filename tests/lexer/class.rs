// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use miri::lexer::Token;

use super::utils::*;

// ===== Basic Token Tests =====

#[test]
fn test_class_keyword() {
    lexer_test("class", vec![Token::Class]);
}

#[test]
fn test_trait_keyword() {
    lexer_test("trait", vec![Token::Trait]);
}

#[test]
fn test_super_keyword() {
    lexer_test("super", vec![Token::Super]);
}

// ===== Keyword Context Tests =====

#[test]
fn test_class_in_contexts() {
    // `class` as keyword vs as part of identifier
    lexer_test(
        "class class_name classname name_class",
        vec![
            Token::Class,
            Token::Identifier, // class_name
            Token::Identifier, // classname
            Token::Identifier, // name_class
        ],
    );
}

#[test]
fn test_trait_in_contexts() {
    lexer_test(
        "trait trait_impl traitimpl impl_trait",
        vec![
            Token::Trait,
            Token::Identifier,
            Token::Identifier,
            Token::Identifier,
        ],
    );
}

#[test]
fn test_super_in_contexts() {
    lexer_test(
        "super super_call supercall call_super",
        vec![
            Token::Super,
            Token::Identifier,
            Token::Identifier,
            Token::Identifier,
        ],
    );
}

// ===== Class Declaration Tokenization =====

#[test]
fn test_simple_class_declaration() {
    lexer_test("class Graph", vec![Token::Class, Token::Identifier]);
}

#[test]
fn test_class_with_extends() {
    lexer_test(
        "class Graph extends BaseGraph",
        vec![
            Token::Class,
            Token::Identifier, // Graph
            Token::Extends,
            Token::Identifier, // BaseGraph
        ],
    );
}

#[test]
fn test_class_with_implements() {
    lexer_test(
        "class Graph implements Traversable",
        vec![
            Token::Class,
            Token::Identifier, // Graph
            Token::Implements,
            Token::Identifier, // Traversable
        ],
    );
}

#[test]
fn test_class_with_extends_and_implements() {
    lexer_test(
        "class Graph extends Base implements Trait1, Trait2",
        vec![
            Token::Class,
            Token::Identifier, // Graph
            Token::Extends,
            Token::Identifier, // Base
            Token::Implements,
            Token::Identifier, // Trait1
            Token::Comma,
            Token::Identifier, // Trait2
        ],
    );
}

#[test]
fn test_class_with_generics() {
    lexer_test(
        "class Graph<T>",
        vec![
            Token::Class,
            Token::Identifier, // Graph
            Token::LessThan,
            Token::Identifier, // T
            Token::GreaterThan,
        ],
    );
}

#[test]
fn test_class_with_generic_constraint() {
    lexer_test(
        "class Graph<T extends Comparable>",
        vec![
            Token::Class,
            Token::Identifier, // Graph
            Token::LessThan,
            Token::Identifier, // T
            Token::Extends,
            Token::Identifier, // Comparable
            Token::GreaterThan,
        ],
    );
}

// ===== Trait Declaration Tokenization =====

#[test]
fn test_simple_trait_declaration() {
    lexer_test("trait Drawable", vec![Token::Trait, Token::Identifier]);
}

#[test]
fn test_trait_with_multiple_extends() {
    lexer_test(
        "trait Drawable extends Renderable, Colorable",
        vec![
            Token::Trait,
            Token::Identifier, // Drawable
            Token::Extends,
            Token::Identifier, // Renderable
            Token::Comma,
            Token::Identifier, // Colorable
        ],
    );
}

// ===== Super Expression Tokenization =====

#[test]
fn test_super_dot_init() {
    lexer_test(
        "super.init()",
        vec![
            Token::Super,
            Token::Dot,
            Token::Identifier, // init
            Token::LParen,
            Token::RParen,
        ],
    );
}

#[test]
fn test_super_method_call() {
    lexer_test(
        "super.method(arg)",
        vec![
            Token::Super,
            Token::Dot,
            Token::Identifier, // method
            Token::LParen,
            Token::Identifier, // arg
            Token::RParen,
        ],
    );
}

// ===== Class Body Tokenization =====

#[test]
fn test_class_field_declaration() {
    lexer_test(
        "let _graph Map<String, List<String>>",
        vec![
            Token::Let,
            Token::Identifier, // _graph
            Token::Identifier, // Map
            Token::LessThan,
            Token::Identifier, // String
            Token::Comma,
            Token::Identifier, // List
            Token::LessThan,
            Token::Identifier, // String
            Token::GreaterThan,
            Token::GreaterThan,
        ],
    );
}

#[test]
fn test_visibility_modifiers_in_class() {
    lexer_test(
        "public fn traverse()",
        vec![
            Token::Public,
            Token::Fn,
            Token::Identifier, // traverse
            Token::LParen,
            Token::RParen,
        ],
    );

    lexer_test(
        "private let _data int",
        vec![
            Token::Private,
            Token::Let,
            Token::Identifier, // _data
            Token::Identifier, // int
        ],
    );

    lexer_test(
        "protected fn helper()",
        vec![
            Token::Protected,
            Token::Fn,
            Token::Identifier, // helper
            Token::LParen,
            Token::RParen,
        ],
    );
}

// ===== Indentation Tests =====

#[test]
fn test_class_body_indentation() {
    let source = "class Graph\n    let x int";
    lexer_test(
        source,
        vec![
            Token::Class,
            Token::Identifier, // Graph
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Let,
            Token::Identifier, // x
            Token::Identifier, // int
            Token::Dedent,
        ],
    );
}

#[test]
fn test_class_multiple_members_indentation() {
    let source = "class Graph\n    let x int\n    fn init()";
    lexer_test(
        source,
        vec![
            Token::Class,
            Token::Identifier, // Graph
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Let,
            Token::Identifier, // x
            Token::Identifier, // int
            Token::ExpressionStatementEnd,
            Token::Fn,
            Token::Identifier, // init
            Token::LParen,
            Token::RParen,
            Token::Dedent,
        ],
    );
}

#[test]
fn test_nested_indentation_in_class() {
    let source = "class Graph\n    fn init()\n        let x = 1";
    lexer_test(
        source,
        vec![
            Token::Class,
            Token::Identifier, // Graph
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Fn,
            Token::Identifier, // init
            Token::LParen,
            Token::RParen,
            Token::ExpressionStatementEnd,
            Token::Indent,
            Token::Let,
            Token::Identifier, // x
            Token::Assign,
            Token::Int,
            Token::Dedent,
            Token::Dedent,
        ],
    );
}

// ===== Case Sensitivity Tests =====

#[test]
fn test_class_case_sensitivity() {
    // Keywords are lowercase only
    lexer_test("Class CLASS", vec![Token::Identifier, Token::Identifier]);
}

#[test]
fn test_trait_case_sensitivity() {
    lexer_test("Trait TRAIT", vec![Token::Identifier, Token::Identifier]);
}

#[test]
fn test_super_case_sensitivity() {
    lexer_test("Super SUPER", vec![Token::Identifier, Token::Identifier]);
}

// ===== Boundary Tests =====

#[test]
fn test_keyword_operator_boundary() {
    lexer_test(
        "class(x)",
        vec![
            Token::Class,
            Token::LParen,
            Token::Identifier,
            Token::RParen,
        ],
    );
    lexer_test("super.x", vec![Token::Super, Token::Dot, Token::Identifier]);
}
