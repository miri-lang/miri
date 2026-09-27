// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A body lowered for one instantiation leaves no parameter open, and names
//! the ones its substitution bound; a shared generic body leaves its own
//! parameters open, and a closure inside it inherits them. Code generation
//! releases a value at an open instantiation through the shared drop function
//! only when the body leaves that parameter open, so a missed substitution in
//! an instantiated body is reported rather than silently shared.

use miri::ast::statement::StatementKind as AstStatementKind;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::lowering::{lower_function, lower_generic_instantiation};
use miri::mir::Body;
use miri::pipeline::Pipeline;
use std::collections::{HashMap, HashSet};

const PICK: &str = r#"
fn pick<T>(x T) T
    let f = fn() int
        return 1
    return x

fn main()
    let s = pick("a" + "b")
    println(s)
"#;

/// The bodies lowering `pick` gives: its own, and its closure's.
fn lower_pick(subs: Option<HashMap<String, Type>>) -> (Body, Vec<Body>) {
    let result = Pipeline::new()
        .frontend(PICK)
        .expect("the program type-checks");
    let pick = result
        .ast
        .body
        .iter()
        .find(|stmt| {
            matches!(&stmt.node, AstStatementKind::FunctionDeclaration(func) if func.name == "pick")
        })
        .expect("`pick` is declared");
    let (body, lambdas) = match subs {
        None => lower_function(pick, &result.type_checker, false, false),
        Some(subs) => lower_generic_instantiation(pick, &result.type_checker, false, false, &subs),
    }
    .expect("`pick` lowers");
    (
        body,
        lambdas.into_iter().map(|lambda| lambda.body).collect(),
    )
}

fn names(list: &[&str]) -> HashSet<String> {
    list.iter().map(|name| name.to_string()).collect()
}

#[test]
fn a_body_lowered_for_one_instantiation_leaves_no_parameter_open() {
    let string = Type::new(TypeKind::String, Span::default());
    let (body, lambdas) = lower_pick(Some(HashMap::from([("T".to_string(), string)])));

    assert!(body.open_params.is_empty(), "{:?}", body.open_params);
    assert_eq!(body.bound_params, names(&["T"]));
    for lambda in &lambdas {
        assert!(lambda.open_params.is_empty(), "{:?}", lambda.open_params);
        assert_eq!(lambda.bound_params, names(&["T"]));
    }
}

#[test]
fn a_shared_generic_body_and_its_closures_leave_its_parameters_open() {
    let (body, lambdas) = lower_pick(None);

    assert_eq!(body.open_params, names(&["T"]));
    assert!(body.bound_params.is_empty());
    assert!(!lambdas.is_empty(), "the closure inside `pick` is lowered");
    for lambda in &lambdas {
        assert_eq!(lambda.open_params, names(&["T"]));
    }
}

#[test]
fn a_body_substituted_at_an_open_parameter_leaves_that_parameter_open() {
    // A shared body calling `pick` at its own parameter reaches `pick` at a
    // type that still names one: the callee is shared too.
    let open = Type::new(
        TypeKind::Generic(
            "U".to_string(),
            None,
            miri::ast::types::TypeDeclarationKind::None,
        ),
        Span::default(),
    );
    let (body, _) = lower_pick(Some(HashMap::from([("T".to_string(), open)])));

    assert_eq!(body.open_params, names(&["U"]));
    assert!(body.bound_params.is_empty(), "{:?}", body.bound_params);
}
