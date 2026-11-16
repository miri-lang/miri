// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How a call to a generic runtime entry point is typed, read off a program
//! whose recorded types are then made inconsistent the way only a defect
//! upstream could make them.

use crate::ast::statement::StatementKind;
use crate::ast::types::{Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::lowering::lower_function;
use crate::pipeline::{Pipeline, PipelineResult};

const GENERIC_ENTRY: &str = r#"
@element("item")
@returns_element
runtime "core" fn miri_rt_x<T>(item T) T

fn main()
    let y = miri_rt_x(1)
"#;

/// The checked program, with every recorded type of the generic entry point
/// itself passed to `rewrite`, which keeps it (`Some`) or drops it (`None`).
fn checked_with_entry_type(rewrite: fn(Type) -> Option<Type>) -> PipelineResult {
    let Ok(mut result) = Pipeline::new().frontend(GENERIC_ENTRY) else {
        panic!("the program checks");
    };
    let types = std::mem::take(&mut result.type_checker.type_table.types);
    result.type_checker.type_table.types = (types.into_iter())
        .filter_map(|(id, ty)| {
            let is_entry = matches!(&ty.kind, TypeKind::Function(signature)
                if signature.generics.is_some());
            if is_entry {
                return rewrite(ty).map(|ty| (id, ty));
            }
            Some((id, ty))
        })
        .collect();
    result
}

/// The debug text of the error lowering `main` fails with.
fn lowering_error(result: &PipelineResult) -> String {
    let main = (result.ast.body.iter()).find(
        |stmt| matches!(&stmt.node, StatementKind::FunctionDeclaration(f) if f.name == "main"),
    );
    let Some(main) = main else {
        panic!("main is declared");
    };
    match lower_function(main, &result.type_checker, false, false) {
        Ok(_) => panic!("the call lowered with its entry point untyped"),
        Err(error) => format!("{error:?}"),
    }
}

#[test]
fn a_generic_runtime_call_with_no_recorded_type_is_an_internal_error() {
    let result = checked_with_entry_type(|_| None);
    let text = lowering_error(&result);
    assert!(text.contains("miri_rt_x"), "{text}");
    assert!(text.contains("no type recorded"), "{text}");
}

#[test]
fn a_generic_runtime_call_recorded_as_no_function_is_an_internal_error() {
    let result = checked_with_entry_type(|_| Some(Type::new(TypeKind::Int, Span::default())));
    let text = lowering_error(&result);
    assert!(text.contains("miri_rt_x"), "{text}");
    assert!(text.contains("not a function"), "{text}");
}
