// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::statement::StatementKind as AstStatementKind;
use miri::mir::lowering::lower_function;
use miri::pipeline::Pipeline;

fn get_main_mir(source: &str, is_release: bool) -> miri::mir::Body {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("Frontend failed");

    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|stmt| {
            if let AstStatementKind::FunctionDeclaration(name, ..) = &stmt.node {
                name == "main"
            } else {
                false
            }
        })
        .expect("No main function found");

    lower_function(func_stmt, &result.type_checker, is_release, false).expect("Lowering failed")
}

#[test]
fn test_debug_names_present_in_debug_build() {
    let source = "fn main(): let x = 5";
    let body = get_main_mir(source, false);

    // Should find local with name "x"
    let has_x = body
        .local_decls
        .iter()
        .any(|d| d.name.as_deref() == Some("x"));
    assert!(has_x, "Variable 'x' should have a name in debug build");
}

#[test]
fn test_debug_names_stripped_in_release_build() {
    let source = "fn main(): let x = 5";
    let body = get_main_mir(source, true);

    // Should NOT find local with name "x"
    let has_x = body
        .local_decls
        .iter()
        .any(|d| d.name.as_deref() == Some("x"));
    assert!(
        !has_x,
        "Variable 'x' should NOT have a name in release build"
    );

    // Check that we still have locals, just unnamed (except ret val which is unnamed anyway usually)
    // _0 is ret, _1 is x
    assert!(body.local_decls.len() >= 2);
    // Ensure user variable has no name
    assert!(body.local_decls[1].name.is_none());
}
