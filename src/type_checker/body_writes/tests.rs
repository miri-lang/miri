// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The mentions one walk records, read off small functions.

use super::*;
use crate::ast::Program;
use crate::lexer::Lexer;
use crate::parser::Parser;

/// The mentions of the binding `name` in the body itself, outside any
/// closure or nested function.
fn of_binding<'s, 'b>(
    uses: &'s BodyWrites<'b>,
    name: &'s str,
) -> impl Iterator<Item = &'s Mention<'b>> + 's {
    (uses.mentions.iter())
        .filter(move |mention| mention.scope == Scope::Body && mention.name() == name)
}

fn parse(source: &str) -> Program {
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new(&mut lexer, source);
    match parser.parse() {
        Ok(program) => program,
        Err(error) => panic!("the fixture should parse: {error:?}"),
    }
}

/// The body of the function `name` the program declares at its top level.
fn body_of<'p>(program: &'p Program, name: &str) -> &'p Statement {
    let mut pending: Vec<&Statement> = program.body.iter().collect();
    while let Some(statement) = pending.pop() {
        match &statement.node {
            StatementKind::FunctionDeclaration(declaration) if declaration.name == name => {
                if let Some(body) = declaration.body.as_deref() {
                    return body;
                }
            }
            StatementKind::Block(statements) => pending.extend(statements),
            _ => {}
        }
    }
    panic!("the fixture declares `{name}`")
}

/// What each mention of `binding` in the body itself is, as a short word.
fn roles_of(source: &str, function: &str, binding: &str) -> Vec<&'static str> {
    let program = parse(source);
    let uses = BodyWrites::of(body_of(&program, function));
    let words = of_binding(&uses, binding).map(|mention| match mention.role {
        Some(Role::Assigned) => "assigned",
        Some(Role::Receiver { .. }) => "receiver",
        Some(Role::Argument {
            through_cast: true, ..
        }) => "cast-argument",
        Some(Role::Argument { is_bare: true, .. }) => "argument",
        Some(Role::Argument { .. }) => "path-argument",
        None if mention.place == Place::Read => "read",
        None => "value",
    });
    words.collect()
}

#[test]
fn a_store_through_a_parameter_starts_an_assigned_path() {
    let source = r#"
fn f(xs [int], p Point)
    xs[0] = 1
    p.x = 2
"#;
    assert_eq!(roles_of(source, "f", "xs"), ["assigned"]);
    assert_eq!(roles_of(source, "f", "p"), ["assigned"]);
}

#[test]
fn a_method_call_makes_the_root_of_its_receiver_path_a_receiver() {
    let source = r#"
fn f(xs [int], h Holder)
    xs.push(1)
    h.items.push(2)
"#;
    assert_eq!(roles_of(source, "f", "xs"), ["receiver"]);
    assert_eq!(roles_of(source, "f", "h"), ["receiver"]);
}

#[test]
fn an_argument_is_seen_through_its_label_but_a_cast_is_marked() {
    let source = r#"
fn f(xs [int], h Holder, ys [int])
    g(xs)
    g(h.items)
    g(ys as [int])
"#;
    assert_eq!(roles_of(source, "f", "xs"), ["argument"]);
    assert_eq!(roles_of(source, "f", "h"), ["path-argument"]);
    assert_eq!(roles_of(source, "f", "ys"), ["cast-argument"]);
}

#[test]
fn reads_are_told_from_other_uses() {
    let source = r#"
fn f(xs [int]) int
    let n = xs[0] + xs.length()
    let copy = xs
    return xs[n]
"#;
    assert_eq!(
        roles_of(source, "f", "xs"),
        ["read", "receiver", "value", "read"]
    );
}

#[test]
fn a_closure_or_nested_function_is_not_the_body_itself() {
    let source = r#"
fn f(xs [int])
    let push = fn(): xs.push(1)
    fn inner()
        xs.push(2)
"#;
    let program = parse(source);
    let uses = BodyWrites::of(body_of(&program, "f"));
    assert_eq!(of_binding(&uses, "xs").count(), 0);
    let scopes: Vec<Scope> = (uses.mentions.iter())
        .filter(|mention| mention.name() == "xs")
        .map(|mention| mention.scope)
        .collect();
    assert_eq!(scopes, [Scope::Closure, Scope::Nested]);
}
