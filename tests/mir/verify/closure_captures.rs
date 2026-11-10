// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rules that a closure is built from the captures its body
//! reads, and that nothing projects into a value of a function type.
//!
//! Each broken fixture is paired with its corrected form, so a rule that
//! reports nothing at all cannot pass.

use miri::ast::type_arguments::{TypeArgKind, TypeArgument};
use miri::ast::types::{FunctionTypeData, Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::body::{Body, ExecutionModel, LocalDecl};
use miri::mir::symbol::{ClosureKind, Symbol};
use miri::mir::verify::{
    verify_backend_bodies, verify_closure_captures, verify_function_projections,
    VerificationViolation,
};
use miri::mir::{
    AggregateKind, BasicBlockData, Local, Operand, Place, PlaceElem, Rvalue, Statement,
    StatementKind, TerminatorKind,
};
use miri::pipeline::Pipeline;

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn function_type() -> Type {
    ty(TypeKind::Function(Box::new(FunctionTypeData {
        generics: None,
        params: Vec::new(),
        return_type: None,
    })))
}

fn closure_symbol() -> Symbol {
    Symbol::closure(ClosureKind::Lambda, 1, std::iter::empty::<&Type>())
}

fn messages(violations: &[VerificationViolation]) -> String {
    violations
        .iter()
        .map(VerificationViolation::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// A body holding one statement, `kind`, over locals of `locals`.
fn body_with(locals: &[Type], kind: StatementKind) -> Body {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    for local in locals {
        body.new_local(LocalDecl::new(local.clone(), Span::default()));
    }
    let mut block = BasicBlockData::new(None);
    block.statements.push(Statement {
        kind,
        span: Span::default(),
    });
    body.basic_blocks.push(block);
    body
}

/// A lambda body reading captures of `captures`, after its return slot and
/// its environment pointer.
fn lambda_reading(captures: &[TypeKind]) -> Body {
    let mut body = Body::new(1, Span::default(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(ty(TypeKind::Int), Span::default()));
    body.new_local(LocalDecl::new(ty(TypeKind::RawPtr), Span::default()));
    for capture in captures {
        let local = body.new_local(LocalDecl::new(ty(capture.clone()), Span::default()));
        body.captures.env_capture_locals.push(local);
    }
    body
}

/// A body building the closure into its first local from its locals of
/// `operands`, one operand each, in order.
fn creating(operands: &[TypeKind]) -> Body {
    let locals: Vec<Type> = std::iter::once(function_type())
        .chain(operands.iter().cloned().map(ty))
        .collect();
    let built = Rvalue::Aggregate(
        AggregateKind::Closure(closure_symbol(), function_type()),
        (1..=operands.len())
            .map(|local| Operand::Copy(Place::new(Local(local))))
            .collect(),
    );
    body_with(&locals, StatementKind::Assign(Place::new(Local(0)), built))
}

/// A body rebuilding the closure into its first local, already built, from
/// its locals of `operands`, one operand each, in order.
fn recreating(operands: &[TypeKind]) -> Body {
    let mut body = creating(operands);
    for statement in &mut body.basic_blocks[0].statements {
        if let StatementKind::Assign(place, rvalue) = &statement.kind {
            statement.kind = StatementKind::Reassign(place.clone(), rvalue.clone());
        }
    }
    body
}

/// What the capture rule finds over a program of `creator` and `lambda`.
fn capture_findings(creator: Body, lambda: Body) -> Vec<VerificationViolation> {
    let program = vec![
        ("main".to_string(), creator),
        (closure_symbol().link_name(), lambda),
    ];
    verify_closure_captures(&program)
        .into_iter()
        .map(|(_, violation)| violation)
        .collect()
}

#[test]
fn a_closure_built_from_its_captures_raises_nothing() {
    let captures = [TypeKind::String, TypeKind::Int];
    let found = capture_findings(creating(&captures), lambda_reading(&captures));
    assert!(found.is_empty(), "{}", messages(&found));
}

#[test]
fn a_closure_built_from_fewer_operands_than_its_body_reads_is_flagged() {
    let found = capture_findings(
        creating(&[TypeKind::String]),
        lambda_reading(&[TypeKind::String, TypeKind::Int]),
    );
    let text = messages(&found);
    assert_eq!(found.len(), 1, "{text}");
    assert!(
        text.contains("built from 1 operand(s), but its body reads 2 capture(s)"),
        "{text}"
    );
}

#[test]
fn a_counted_operand_for_an_uncounted_capture_is_flagged() {
    let found = capture_findings(
        creating(&[TypeKind::Int, TypeKind::String]),
        lambda_reading(&[TypeKind::Int, TypeKind::Int]),
    );
    let text = messages(&found);
    assert_eq!(found.len(), 1, "{text}");
    assert!(
        text.contains("capture 1") && text.contains("a counted") && text.contains("an uncounted"),
        "{text}"
    );
}

#[test]
fn an_uncounted_operand_for_a_counted_capture_is_flagged() {
    let found = capture_findings(
        creating(&[TypeKind::Int]),
        lambda_reading(&[TypeKind::String]),
    );
    assert_eq!(found.len(), 1, "{}", messages(&found));
}

#[test]
fn a_narrower_operand_for_a_wider_capture_is_flagged() {
    let found = capture_findings(
        creating(&[TypeKind::I32]),
        lambda_reading(&[TypeKind::I128]),
    );
    let text = messages(&found);
    assert_eq!(found.len(), 1, "{text}");
    assert!(
        text.contains("capture 0") && text.contains("held differently"),
        "{text}"
    );
}

#[test]
fn an_operand_held_as_it_is_read_raises_nothing() {
    let found = capture_findings(
        creating(&[TypeKind::I128, TypeKind::F32]),
        lambda_reading(&[TypeKind::I128, TypeKind::F32]),
    );
    assert!(found.is_empty(), "{}", messages(&found));
}

#[test]
fn a_closure_whose_body_is_not_compiled_is_flagged() {
    let program = vec![("main".to_string(), creating(&[TypeKind::String]))];
    let found: Vec<VerificationViolation> = verify_closure_captures(&program)
        .into_iter()
        .map(|(_, violation)| violation)
        .collect();
    let text = messages(&found);
    assert_eq!(found.len(), 1, "{text}");
    assert!(text.contains("no compiled body"), "{text}");
}

#[test]
fn a_closure_rebuilt_into_its_place_is_checked_too() {
    let found = capture_findings(
        recreating(&[TypeKind::String]),
        lambda_reading(&[TypeKind::String, TypeKind::Int]),
    );
    let text = messages(&found);
    assert_eq!(found.len(), 1, "{text}");
    assert!(text.contains("built from 1 operand(s)"), "{text}");
}

/// A body copying field `projection` of its function-typed first local into
/// its second.
fn reading_closure(projection: Vec<PlaceElem>) -> Body {
    let read = Place {
        local: Local(0),
        projection,
    };
    body_with(
        &[function_type(), ty(TypeKind::Int)],
        StatementKind::Assign(Place::new(Local(1)), Rvalue::Use(Operand::Copy(read))),
    )
}

#[test]
fn a_field_projection_on_a_closure_is_flagged() {
    let found = verify_function_projections(&reading_closure(vec![PlaceElem::Field(0)]));
    let text = messages(&found);
    assert_eq!(found.len(), 1, "{text}");
    assert!(
        text.contains("projects into a value of a function type"),
        "{text}"
    );
}

#[test]
fn a_whole_closure_read_raises_nothing() {
    let found = verify_function_projections(&reading_closure(Vec::new()));
    assert!(found.is_empty(), "{}", messages(&found));
}

/// A tuple whose first element is a function value.
fn tuple_holding_a_closure() -> Type {
    let element =
        |ty: Type| TypeArgument::new(0, TypeArgKind::Type(Box::new(ty), false), Span::default());
    ty(TypeKind::Tuple(vec![
        element(function_type()),
        element(ty(TypeKind::Int)),
    ]))
}

#[test]
fn a_projection_into_a_closure_held_in_a_tuple_is_flagged() {
    let read = Place {
        local: Local(0),
        projection: vec![PlaceElem::Field(0), PlaceElem::Field(0)],
    };
    let body = body_with(
        &[tuple_holding_a_closure(), ty(TypeKind::Int)],
        StatementKind::Assign(Place::new(Local(1)), Rvalue::Use(Operand::Copy(read))),
    );
    let found = verify_function_projections(&body);
    let text = messages(&found);
    assert_eq!(found.len(), 1, "{text}");
    assert!(
        text.contains("projects into a value of a function type"),
        "{text}"
    );
}

#[test]
fn reading_a_closure_out_of_a_tuple_raises_nothing() {
    let read = Place {
        local: Local(0),
        projection: vec![PlaceElem::Field(0)],
    };
    let body = body_with(
        &[tuple_holding_a_closure(), function_type()],
        StatementKind::Assign(Place::new(Local(1)), Rvalue::Use(Operand::Copy(read))),
    );
    let found = verify_function_projections(&body);
    assert!(found.is_empty(), "{}", messages(&found));
}

const CLOSURE_PROGRAM: &str = "
fn main()
    let s = f\"a{1}\"
    let f = fn() int: s.length()
    println(f\"{f()}\")
";

/// What the backend checks report over `CLOSURE_PROGRAM` once `main` is
/// changed by `mutate`.
fn backend_lines_after(mutate: impl FnOnce(&mut Body)) -> Vec<String> {
    let (mut bodies, facts) = Pipeline::new()
        .get_mir_program_with_rc(CLOSURE_PROGRAM)
        .expect("the program lowers");
    let (_, main) = (bodies.iter_mut())
        .find(|(name, _)| name == "main")
        .expect("the program has a main");
    mutate(main);
    let result = Pipeline::new()
        .frontend_script(CLOSURE_PROGRAM)
        .expect("the program checks");
    verify_backend_bodies(
        result.type_checker.type_definitions(),
        &bodies,
        &facts,
        None,
    )
    .expect("the program's ownership facts are current")
    .lines
}

/// Give the closure `main` builds one operand more than its body reads.
fn add_a_capture_operand(main: &mut Body) {
    let operands = (main.basic_blocks.iter_mut())
        .flat_map(|block| &mut block.statements)
        .find_map(|statement| match &mut statement.kind {
            StatementKind::Assign(_, Rvalue::Aggregate(AggregateKind::Closure(..), operands)) => {
                Some(operands)
            }
            _ => None,
        })
        .expect("main builds a closure");
    let extra = operands[0].clone();
    operands.push(extra);
}

/// Call the closure `main` holds through a field projection of it.
fn call_through_a_projection(main: &mut Body) {
    let callee = (main.basic_blocks.iter_mut())
        .filter_map(|block| block.terminator.as_mut())
        .find_map(|terminator| match &mut terminator.kind {
            TerminatorKind::Call {
                func: Operand::Copy(place) | Operand::Move(place),
                ..
            } => Some(place),
            _ => None,
        })
        .expect("main calls the closure it holds");
    callee.projection.push(PlaceElem::Field(0));
}

#[test]
fn the_backend_checks_pass_a_closure_program() {
    assert_eq!(backend_lines_after(|_| ()), Vec::<String>::new());
}

#[test]
fn the_backend_checks_report_a_closure_built_from_the_wrong_operands() {
    let lines = backend_lines_after(add_a_capture_operand).join("\n");
    assert!(lines.contains("operand(s), but its body reads"), "{lines}");
}

#[test]
fn the_backend_checks_report_a_projection_into_a_closure() {
    let lines = backend_lines_after(call_through_a_projection).join("\n");
    assert!(
        lines.contains("projects into a value of a function type"),
        "{lines}"
    );
}
