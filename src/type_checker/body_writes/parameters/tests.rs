// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which parameters a body writes, decided by the type checker over small
//! programs written against the standard library.

use crate::ast::common::Parameter;
use crate::ast::expression::ExpressionKind;
use crate::ast::statement::{Statement, StatementKind};
use crate::pipeline::{Pipeline, PipelineResult};

fn checked(source: &str) -> PipelineResult {
    match Pipeline::new().frontend(source) {
        Ok(result) => result,
        Err(error) => panic!("the fixture should type-check: {error:?}"),
    }
}

/// Every parameter `statements` declare, with the function or closure it
/// belongs to: a declared function by its name, a closure by the name of
/// the binding it initializes.
fn parameters<'s>(statements: &'s [Statement], found: &mut Vec<(String, &'s Parameter)>) {
    for statement in statements {
        match &statement.node {
            StatementKind::FunctionDeclaration(declaration) => {
                let owner = &declaration.name;
                found.extend(declaration.params.iter().map(|p| (owner.clone(), p)));
                if let Some(body) = declaration.body.as_deref() {
                    parameters(std::slice::from_ref(body), found);
                }
            }
            StatementKind::Block(statements) => parameters(statements, found),
            StatementKind::Class(class) => parameters(&class.body, found),
            StatementKind::Variable(declarations, _) => {
                for declaration in declarations {
                    let Some(initializer) = declaration.initializer.as_deref() else {
                        continue;
                    };
                    if let ExpressionKind::Lambda(lambda) = &initializer.node {
                        let owner = &declaration.name;
                        found.extend(lambda.params.iter().map(|p| (owner.clone(), p)));
                        parameters(std::slice::from_ref(&*lambda.body), found);
                    }
                }
            }
            _ => {}
        }
    }
}

/// `function.parameter` for each parameter the type checker finds written,
/// in declaration order.
fn written(source: &str) -> Vec<String> {
    let result = checked(source);
    let mut found = Vec::new();
    parameters(&result.ast.body, &mut found);
    let writes = &result.type_checker.parameter_writes;
    (found.into_iter())
        .filter(|(_, param)| writes.writes(param))
        .map(|(owner, param)| format!("{owner}.{}", param.name))
        .collect()
}

/// A list parameter is written exactly when the body calls on it a method
/// the standard library's own body writes its receiver with.
#[test]
fn a_list_parameter_is_written_through_the_standard_library_writers_only() {
    let source = r#"
use system.collections.list

fn writer(xs [int])
    xs.push(1)
    xs.reverse()

fn reader(xs [int]) int
    xs.length() + xs[0]

fn main()
    writer(List([1]))
    reader(List([1]))
"#;
    assert_eq!(written(source), ["writer.xs"]);
}

/// A write through a field of a parameter writes the parameter; a read
/// through the same path does not.
#[test]
fn a_writer_called_through_a_field_writes_the_parameter_the_path_starts_at() {
    let source = r#"
use system.collections.list

struct Holder
    items [int]

fn writes(h Holder)
    h.items.push(1)

fn reads(h Holder) int
    h.items.length()

fn main()
    writes(Holder(items: List([1])))
    reads(Holder(items: List([1])))
"#;
    assert_eq!(written(source), ["writes.h"]);
}

#[test]
fn closures_and_nested_functions_are_judged_against_their_own_bodies() {
    let source = r#"
use system.collections.list

fn outer(xs [int]) int
    let add = fn(ys [int]): ys.push(1)
    fn inner(zs [int])
        zs.push(2)
    xs.length()

fn main()
    outer(List([1]))
"#;
    assert_eq!(written(source), ["add.ys", "inner.zs"]);
}

/// Handing an `out` parameter on as another call's `out` argument writes
/// it; a lent parameter is never written by being read, and the checker
/// refuses one as an `out` argument, so only an `out` parameter's verdict
/// reads the `out` rule.
#[test]
fn an_out_parameter_handed_on_as_an_out_argument_is_written() {
    let source = r#"
fn set(x out int) int
    x = 5
    return 0

fn pass(n out int) int
    return set(n)

fn reads(n int) int
    return n + 1

fn main()
    var k = 1
    let r = pass(k) + reads(k)
"#;
    assert_eq!(written(source), ["set.x", "pass.n"]);
}
