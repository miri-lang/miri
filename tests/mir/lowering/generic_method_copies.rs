// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A method declaring type parameters of its own has no body shared by every
//! type argument: only a copy lowered at a call's arguments exists, and
//! lowering one with a parameter pinned to nothing is an internal error.

use miri::ast::expression::ExpressionKind;
use miri::ast::statement::{Statement, StatementKind};
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::lowering::lower_class_method;
use miri::pipeline::Pipeline;

const SOURCE: &str = "
class Box
    fn same<T>(x T) T
        return x

fn main()
    let b = Box()
    let n = b.same(5)
";

/// The statement declaring `method` in the class `class`.
fn class_method<'a>(ast: &'a [Statement], class: &str, method: &str) -> Option<&'a Statement> {
    ast.iter()
        .find_map(|stmt| match &stmt.node {
            StatementKind::Class(data) => matches!(
                &data.name.node,
                ExpressionKind::Identifier(name, _) if name == class
            )
            .then_some(data),
            _ => None,
        })?
        .body
        .iter()
        .find(|member| {
            matches!(&member.node, StatementKind::FunctionDeclaration(f) if f.name == method)
        })
}

#[test]
fn lowering_a_generic_method_as_a_shared_body_is_an_internal_error() {
    let result = Pipeline::new().frontend(SOURCE).expect("frontend failed");
    let method = class_method(&result.ast.body, "Box", "same").expect("Box.same not parsed");
    let receiver = Type::new(TypeKind::Custom("Box".to_string(), None), Span::default());
    let error = lower_class_method(method, receiver, &result.type_checker, false)
        .expect_err("a generic method has no shared body to lower");
    let message = error.to_string();
    assert!(
        message.contains("its own type parameter `T` pinned to nothing"),
        "unexpected error: {message}"
    );
}
