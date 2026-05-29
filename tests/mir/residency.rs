// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

// `gpu let` / `gpu var` lower into MIR `Local`s that carry
// `BindingResidency::Gpu`. The `Body` pretty-printer must show the `gpu`
// keyword so the residency round-trips through Display.

use miri::ast::statement::StatementKind as AstStatementKind;
use miri::mir::body::BindingResidency;
use miri::mir::lowering::lower_function;
use miri::pipeline::Pipeline;

fn get_main_mir(source: &str) -> miri::mir::Body {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("Frontend failed");

    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|stmt| {
            if let AstStatementKind::FunctionDeclaration(func) = &stmt.node {
                func.name == "main"
            } else {
                false
            }
        })
        .expect("No main function found");

    lower_function(func_stmt, &result.type_checker, false, false)
        .expect("Lowering failed")
        .0
}

#[test]
fn gpu_let_stamps_residency_on_local() {
    let body = get_main_mir(
        "
fn main()
    gpu let g = 0
",
    );

    let g_decl = body
        .local_decls
        .iter()
        .find(|d| d.name.as_deref() == Some("g"))
        .expect("Variable 'g' missing from MIR locals");

    assert_eq!(g_decl.residency, BindingResidency::Gpu);
}

#[test]
fn host_let_default_residency_is_host() {
    let body = get_main_mir(
        "
fn main()
    let h = 0
",
    );

    let h_decl = body
        .local_decls
        .iter()
        .find(|d| d.name.as_deref() == Some("h"))
        .expect("Variable 'h' missing from MIR locals");

    assert_eq!(h_decl.residency, BindingResidency::Host);
}

#[test]
fn gpu_var_pretty_print_round_trips_keyword() {
    let body = get_main_mir(
        "
fn main()
    gpu var g = 0
",
    );

    let printed = format!("{}", body);

    assert!(
        printed.contains("gpu let") || printed.contains("gpu var"),
        "Body Display must emit 'gpu' on gpu-resident locals; got:\n{}",
        printed
    );
}
