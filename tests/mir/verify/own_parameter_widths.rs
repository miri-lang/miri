// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rules over a generic runtime entry point's own type
//! parameters: a value fills the parameter its declaration takes at the
//! call's instantiation, as wide as that parameter; every value at one
//! parameter, and the destination, is one width; the recorded instantiation
//! is one substitution of the declaration; and a body compiled for one
//! instantiation records one and leaves no parameter open.

use crate::mir::utils::runtime_callee;
use crate::mir::value_bodies::*;
use miri::ast::factory::{out_parameter, parameter, type_expr_non_null};
use miri::ast::statement::StatementKind as AstStatementKind;
use miri::ast::types::{FunctionTypeData, TypeKind};
use miri::mir::lowering::lower_function;
use miri::mir::symbol::Symbol;
use miri::mir::verify::{
    verify_collection_element_width, verify_own_parameter_instances, VerificationViolation,
};
use miri::mir::{BasicBlock, Body, Operand};
use miri::pipeline::Pipeline;
use miri::runtime_fns::rt;

/// The signature a call of `replace`'s entry point records: its place, its
/// new value and its result at `place`, `new` and `result`.
fn signature(place: TypeKind, new: TypeKind, result: TypeKind) -> TypeKind {
    TypeKind::Function(Box::new(FunctionTypeData {
        generics: None,
        params: vec![
            out_parameter("place".into(), type_expr_non_null(ty(place)), None, None),
            parameter("new".into(), type_expr_non_null(ty(new)), None, None),
        ],
        return_type: Some(Box::new(type_expr_non_null(ty(result)))),
    }))
}

/// A body over `locals` (`_0` the destination, `_1` the place, `_2` the new
/// value) handing them to `replace`'s entry point, whose operand is typed
/// `recorded`.
fn replacing(locals: [TypeKind; 3], recorded: TypeKind) -> Body {
    let mut func = Operand::function(Symbol::runtime(runtime_callee(rt::REPLACE)), span());
    if let Operand::Function(function) = &mut func {
        function.ty = ty(recorded);
    }
    let kind = miri::mir::TerminatorKind::Call {
        func,
        args: vec![copy(1), copy(2)],
        arg_modes: Vec::new(),
        arg_handles: Vec::new(),
        flags: miri::mir::CallFlags::default(),
        destination: place(0),
        target: Some(BasicBlock(1)),
    };
    body_over(&locals, 0, block(Vec::new(), kind))
}

/// A body handing `int`s to `replace`'s entry point recorded at `declared`.
fn replacing_at(declared: TypeKind) -> Body {
    let recorded = signature(declared.clone(), declared.clone(), declared);
    replacing([TypeKind::Int, TypeKind::Int, TypeKind::Int], recorded)
}

fn ints() -> [TypeKind; 3] {
    [TypeKind::Int, TypeKind::Int, TypeKind::Int]
}

fn messages(violations: &[VerificationViolation]) -> String {
    (violations.iter())
        .map(VerificationViolation::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

fn assert_flagged(violations: &[VerificationViolation], fragment: &str) {
    let text = messages(violations);
    assert!(text.contains(fragment), "expected {fragment:?} in: {text}");
}

#[test]
fn a_value_as_wide_as_the_instantiated_parameter_raises_nothing() {
    let violations = verify_collection_element_width(&replacing_at(TypeKind::Int));
    assert!(violations.is_empty(), "{}", messages(&violations));
}

#[test]
fn a_value_narrower_than_the_instantiated_parameter_is_flagged() {
    let violations = verify_collection_element_width(&replacing_at(TypeKind::I32));
    let text = messages(&violations);
    assert_eq!(violations.len(), 2, "one finding per argument: {text}");
    assert!(
        text.contains("declares that slot as i32 of 4 bytes"),
        "{text}"
    );
}

#[test]
fn a_call_recorded_at_one_instantiation_raises_nothing() {
    let violations = verify_own_parameter_instances(&replacing_at(TypeKind::Int));
    assert!(violations.is_empty(), "{}", messages(&violations));
}

#[test]
fn an_instantiation_spelling_two_types_for_one_parameter_is_flagged() {
    let recorded = signature(TypeKind::I32, TypeKind::Int, TypeKind::Int);
    let violations = verify_own_parameter_instances(&replacing(ints(), recorded));
    assert_flagged(&violations, "is no substitution of its declaration");
}

#[test]
fn values_of_two_widths_at_one_parameter_are_flagged() {
    let recorded = signature(TypeKind::Int, TypeKind::Int, TypeKind::Int);
    let locals = [TypeKind::Int, TypeKind::Int, TypeKind::I32];
    let violations = verify_own_parameter_instances(&replacing(locals, recorded));
    assert_flagged(&violations, "at its one type parameter `T`");
}

#[test]
fn a_call_recording_no_instantiation_is_flagged() {
    let violations = verify_own_parameter_instances(&replacing(ints(), TypeKind::Identifier));
    assert_flagged(&violations, "is called with no instantiation recorded");
}

#[test]
fn a_call_leaving_its_own_parameter_open_is_flagged() {
    let open = || TypeKind::Custom("T".into(), None);
    let recorded = signature(open(), open(), open());
    let violations = verify_own_parameter_instances(&replacing(ints(), recorded));
    assert_flagged(
        &violations,
        "left open in a body compiled for one instantiation",
    );
}

#[test]
fn lowering_refuses_a_generic_runtime_call_with_no_instantiation() {
    let source = r#"
@element("item")
@returns_element
runtime "core" fn miri_rt_x<T>(item T) T

fn main()
    let y = miri_rt_x(1)
"#;
    let mut result = Pipeline::new()
        .frontend(source)
        .expect("the program checks");
    result.type_checker.call_generic_mappings.clear();
    let main = (result.ast.body.iter())
        .find(|stmt| matches!(&stmt.node, AstStatementKind::FunctionDeclaration(f) if f.name == "main"))
        .expect("main is declared");
    let Err(error) = lower_function(main, &result.type_checker, false, false) else {
        panic!("a call with no instantiation lowered");
    };
    let text = format!("{error:?}");
    assert!(text.contains("no instantiation"), "{text}");
}
