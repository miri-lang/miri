// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the release of a resource its owner consumes: the consuming site
//! ends the binding's storage, and the release where its scope ends is kept,
//! deleted or guarded by a drop flag according to whether the binding was
//! consumed on no path, on every path or on some paths to it.

use miri::ast::literal::Literal;
use miri::ast::statement::StatementKind as AstStatementKind;
use miri::ast::types::Type;
use miri::ast::types::TypeKind;
use miri::error::syntax::Span;
use miri::mir::block::{BasicBlock, BasicBlockData};
use miri::mir::drop_elaboration::elaborate_drops;
use miri::mir::lowering::lower_function;
use miri::mir::{
    Body, Constant, EndCause, ExecutionModel, Local, LocalDecl, Operand, Place, Rvalue, Statement,
    StatementKind, Terminator, TerminatorKind,
};
use miri::pipeline::Pipeline;

const HANDLE: &str = r#"
class Handle
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

fn close(h Handle) Handle
    return h
"#;

/// The lowered body of `run`, the function each fixture declares.
fn lowered(program: &str) -> Body {
    let source = format!("{HANDLE}\n{program}");
    let result = Pipeline::new()
        .frontend(&source)
        .expect("the fixture type checks");
    let run = (result.ast.body.iter())
        .find(|stmt| matches!(&stmt.node, AstStatementKind::FunctionDeclaration(f) if f.name == "run"))
        .expect("the fixture declares `run`");
    lower_function(run, &result.type_checker, false, false)
        .expect("the fixture lowers")
        .body
}

fn elaborated(program: &str) -> Body {
    let mut body = lowered(program);
    elaborate_drops(&mut body).expect("the fixture elaborates");
    body
}

fn local_named(body: &Body, name: &str) -> Local {
    let index = body
        .local_decls
        .iter()
        .position(|decl| decl.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no local named `{name}` in\n{body}"));
    Local(index)
}

/// The drop flags the body declares, each with the local it guards.
fn drop_flags(body: &Body) -> Vec<(Local, Local)> {
    (body.local_decls.iter().enumerate())
        .filter_map(|(index, decl)| Some((Local(index), decl.drop_flag_of()?)))
        .collect()
}

/// How many `StorageDead`s of `local` the body holds.
fn storage_ends(body: &Body, local: Local) -> usize {
    (body.basic_blocks.iter())
        .flat_map(|block| &block.statements)
        .filter(|statement| {
            matches!(&statement.kind, StatementKind::StorageDead(place, _)
                if place.local == local && place.projection.is_empty())
        })
        .count()
}

/// Whether a statement of `body` assigns the absent value to `local`.
fn assigns_none_to(body: &Body, local: Local) -> bool {
    (body.basic_blocks.iter())
        .flat_map(|block| &block.statements)
        .any(|statement| match &statement.kind {
            StatementKind::Assign(place, Rvalue::Use(Operand::Constant(constant)))
            | StatementKind::Reassign(place, Rvalue::Use(Operand::Constant(constant))) => {
                place.local == local && matches!(constant.literal, Literal::None)
            }
            _ => false,
        })
}

#[test]
fn a_consumed_resource_slot_is_never_assigned_the_absent_value() {
    let body = lowered(
        r#"
fn run()
    let h = Handle(1)
    close(h)
"#,
    );
    let h = local_named(&body, "h");
    assert!(
        !matches!(body.local_decls[h.0].ty.kind, TypeKind::Option(_)),
        "the fixture's binding is not optional"
    );
    assert!(!assigns_none_to(&body, h), "the slot was nulled:\n{body}");
}

#[test]
fn a_resource_dropped_by_hand_is_never_assigned_the_absent_value() {
    let body = lowered(
        r#"
fn run()
    let h = Handle(1)
    h.drop()
"#,
    );
    let h = local_named(&body, "h");
    assert!(!assigns_none_to(&body, h), "the slot was nulled:\n{body}");
}

#[test]
fn a_resource_moved_into_another_binding_is_never_assigned_the_absent_value() {
    let body = lowered(
        r#"
fn run()
    let h = Handle(1)
    let b = h
    println(f"{b.id}")
"#,
    );
    let h = local_named(&body, "h");
    assert!(!assigns_none_to(&body, h), "the slot was nulled:\n{body}");
}

#[test]
fn a_resource_consumed_on_every_path_has_no_flag_and_no_release_at_its_scope_end() {
    let body = elaborated(
        r#"
fn run(flag bool)
    let h = Handle(1)
    if flag
        close(h)
    else
        close(h)
"#,
    );
    let h = local_named(&body, "h");
    assert!(drop_flags(&body).is_empty(), "a flag was declared:\n{body}");
    assert_eq!(
        storage_ends(&body, h),
        2,
        "only the two consuming sites end the binding:\n{body}"
    );
}

#[test]
fn a_resource_never_consumed_has_no_flag() {
    let body = elaborated(
        r#"
fn run(flag bool)
    let h = Handle(1)
    if flag
        println(f"{h.id}")
"#,
    );
    assert!(drop_flags(&body).is_empty(), "a flag was declared:\n{body}");
}

#[test]
fn a_resource_consumed_in_one_arm_is_released_behind_a_flag() {
    let body = elaborated(
        r#"
fn run(flag bool)
    let h = Handle(1)
    if flag
        close(h)
"#,
    );
    let h = local_named(&body, "h");
    assert_eq!(drop_flags(&body).len(), 1, "one flag:\n{body}");
    assert_eq!(drop_flags(&body)[0].1, h, "the flag guards `h`:\n{body}");
    let flag = drop_flags(&body)[0].0;
    let switches_on_flag = (body.basic_blocks.iter())
        .filter_map(|block| block.terminator.as_ref())
        .any(|terminator| {
            matches!(&terminator.kind, TerminatorKind::SwitchInt { discr, .. }
            if discr.place().is_some_and(|place| place.local == flag))
        });
    assert!(switches_on_flag, "no release tests the flag:\n{body}");
}

#[test]
fn a_resource_moved_into_a_binding_in_one_arm_is_released_behind_a_flag() {
    let body = elaborated(
        r#"
fn run(flag bool)
    let h = Handle(1)
    if flag
        let b = h
        println(f"{b.id}")
"#,
    );
    let h = local_named(&body, "h");
    let guarded: Vec<Local> = drop_flags(&body).into_iter().map(|(_, of)| of).collect();
    assert_eq!(guarded, vec![h], "one flag, guarding `h`:\n{body}");
}

/// How many ends of `local` the body spells as `cause`.
fn ends_by(body: &Body, local: Local, cause: EndCause) -> usize {
    (body.basic_blocks.iter())
        .flat_map(|block| &block.statements)
        .filter(|statement| {
            matches!(&statement.kind, StatementKind::StorageDead(place, found)
                if place.local == local && place.projection.is_empty() && *found == cause)
        })
        .count()
}

/// `close` owns the parameter it keeps, so the call takes the binding's own
/// reference by `move`, and the binding's end there releases nothing.
#[test]
fn a_call_owning_the_resource_it_keeps_spells_its_end_as_moved() {
    let body = lowered(
        r#"
fn run()
    let h = Handle(1)
    close(h)
"#,
    );
    let h = local_named(&body, "h");
    assert_eq!(ends_by(&body, h, EndCause::Moved), 1, "{body}");
    assert_eq!(ends_by(&body, h, EndCause::Scope), 1, "{body}");
}

/// A function named as a value lends its parameters, so a call keeping the
/// resource releases the caller's binding where it consumes it.
#[test]
fn a_call_lending_the_resource_it_keeps_spells_its_end_as_consumed() {
    let body = lowered(
        r#"
fn run()
    let f = close
    let h = Handle(1)
    close(h)
"#,
    );
    let h = local_named(&body, "h");
    assert_eq!(ends_by(&body, h, EndCause::Consumed), 1, "{body}");
    assert_eq!(ends_by(&body, h, EndCause::Scope), 1, "{body}");
}

#[test]
fn a_move_into_another_binding_spells_its_end_as_consumed() {
    let body = lowered(
        r#"
fn run()
    let h = Handle(1)
    let b = h
    println(f"{b.id}")
"#,
    );
    let h = local_named(&body, "h");
    assert_eq!(ends_by(&body, h, EndCause::Consumed), 1, "{body}");
}

#[test]
fn a_drop_by_hand_spells_its_end_as_consumed() {
    let body = lowered(
        r#"
fn run()
    let h = Handle(1)
    h.drop()
"#,
    );
    let h = local_named(&body, "h");
    assert_eq!(ends_by(&body, h, EndCause::Consumed), 1, "{body}");
}

/// Whether some statement of `body` rebinds `local`, releasing an old value.
fn rebinds(body: &Body, local: Local) -> bool {
    (body.basic_blocks.iter())
        .flat_map(|block| &block.statements)
        .any(|statement| {
            matches!(&statement.kind, StatementKind::Reassign(place, _) if place.local == local)
        })
}

/// The flag `body` declares for `binding`.
fn flag_of(body: &Body, binding: Local) -> Local {
    (drop_flags(body).into_iter())
        .find(|(_, of)| *of == binding)
        .map(|(flag, _)| flag)
        .unwrap_or_else(|| panic!("no flag guards {binding}:\n{body}"))
}

/// Whether `statement` sets `flag`.
fn sets(statement: &Statement, flag: Local) -> bool {
    matches!(&statement.kind, StatementKind::Assign(place, Rvalue::Use(Operand::Constant(c)))
        if place.local == flag && matches!(c.literal, Literal::Boolean(true)))
}

#[test]
fn a_resource_consumed_before_a_break_on_one_path_is_released_behind_a_flag() {
    let body = elaborated(
        r#"
fn run(flag bool)
    let h = Handle(1)
    while true
        if flag
            close(h)
            break
        break
"#,
    );
    let h = local_named(&body, "h");
    let guarded: Vec<Local> = drop_flags(&body).into_iter().map(|(_, of)| of).collect();
    assert_eq!(guarded, vec![h], "one flag, guarding `h`:\n{body}");
}

#[test]
fn a_resource_consumed_before_an_early_return_needs_no_flag() {
    let body = elaborated(
        r#"
fn run(flag bool)
    let h = Handle(1)
    if flag
        close(h)
        return
    println(f"{h.id}")
"#,
    );
    let h = local_named(&body, "h");
    assert!(drop_flags(&body).is_empty(), "a flag was declared:\n{body}");
    assert_eq!(ends_by(&body, h, EndCause::Moved), 1, "{body}");
    assert_eq!(
        ends_by(&body, h, EndCause::Scope),
        1,
        "only the end the consuming path never reaches is kept:\n{body}"
    );
}

#[test]
fn a_rebinding_after_a_consumption_on_one_path_releases_the_old_value_behind_a_flag() {
    let body = elaborated(
        r#"
fn run(flag bool)
    var h = Handle(1)
    if flag
        close(h)
    h = Handle(2)
    println(f"{h.id}")
"#,
    );
    let h = local_named(&body, "h");
    flag_of(&body, h);
    assert!(
        !rebinds(&body, h),
        "the rebinding still releases an old value unguarded:\n{body}"
    );
}

#[test]
fn a_binding_a_call_writes_has_its_flag_set_where_the_call_returns() {
    let body = elaborated(
        r#"
fn run(flag bool)
    let h = Handle(1)
    let b = close(h)
    if flag
        close(b)
"#,
    );
    let b = local_named(&body, "b");
    let flag = flag_of(&body, b);
    let set_on_return = (body.basic_blocks.iter())
        .filter_map(|block| block.terminator.as_ref())
        .filter(|terminator| {
            terminator
                .written_places()
                .any(|place| *place == Place::new(b))
        })
        .flat_map(Terminator::successors)
        .any(|target| {
            (body.basic_blocks[target.0].statements.first()).is_some_and(|s| sets(s, flag))
        });
    assert!(
        set_on_return,
        "no edge out of the call writing `b` sets its flag:\n{body}"
    );
}

fn span() -> Span {
    Span::new(0, 0)
}

fn statement(kind: StatementKind) -> Statement {
    Statement { kind, span: span() }
}

/// `fn f()` binding a string to `_1`, ending it as `ends` say along one
/// path, then returning.
fn ending(ends: Vec<EndCause>) -> Body {
    let mut body = Body::new(0, span(), ExecutionModel::Cpu);
    body.new_local(LocalDecl::new(Type::new(TypeKind::Void, span()), span()));
    body.new_local(LocalDecl::new(Type::new(TypeKind::String, span()), span()));
    let fresh = Operand::Constant(Box::new(Constant {
        span: span(),
        ty: Type::new(TypeKind::String, span()),
        literal: Literal::String("x".to_string()),
    }));
    let mut statements = vec![statement(StatementKind::Assign(
        Place::new(Local(1)),
        Rvalue::Use(fresh),
    ))];
    statements.extend(
        ends.into_iter()
            .map(|cause| statement(StatementKind::StorageDead(Place::new(Local(1)), cause))),
    );
    body.basic_blocks.push(BasicBlockData {
        statements,
        terminator: Some(Terminator::new(TerminatorKind::Return, span())),
        is_cleanup: false,
    });
    body
}

#[test]
fn a_binding_ended_twice_where_its_scope_ends_on_one_path_is_refused() {
    let mut body = ending(vec![EndCause::Scope, EndCause::Scope]);
    let refused = elaborate_drops(&mut body);
    assert!(
        refused
            .as_ref()
            .is_err_and(|why| why.contains("a second time")),
        "{refused:?}"
    );
}

#[test]
fn a_binding_consumed_twice_on_one_path_is_refused() {
    let mut body = ending(vec![EndCause::Consumed, EndCause::Consumed]);
    assert!(elaborate_drops(&mut body).is_err());
}

#[test]
fn a_binding_consumed_then_ended_by_its_scope_keeps_one_end() {
    let mut body = ending(vec![EndCause::Consumed, EndCause::Scope]);
    assert_eq!(elaborate_drops(&mut body), Ok(()));
    assert_eq!(ends_by(&body, Local(1), EndCause::Scope), 0, "{body}");
    assert_eq!(ends_by(&body, Local(1), EndCause::Consumed), 1, "{body}");
}

#[test]
fn a_binding_ended_once_on_each_of_two_paths_is_kept() {
    let mut body = ending(vec![]);
    let end = |cause| statement(StatementKind::StorageDead(Place::new(Local(1)), cause));
    let returning = |statements| BasicBlockData {
        statements,
        terminator: Some(Terminator::new(TerminatorKind::Return, span())),
        is_cleanup: false,
    };
    body.basic_blocks
        .push(returning(vec![end(EndCause::Scope)]));
    body.basic_blocks
        .push(returning(vec![end(EndCause::Scope)]));
    body.basic_blocks[0].terminator = Some(Terminator::new(
        TerminatorKind::SwitchInt {
            discr: Operand::Copy(Place::new(Local(0))),
            targets: vec![(miri::mir::Discriminant::bool_false(), BasicBlock(1))],
            otherwise: BasicBlock(2),
        },
        span(),
    ));
    assert_eq!(elaborate_drops(&mut body), Ok(()));
    assert_eq!(ends_by(&body, Local(1), EndCause::Scope), 2, "{body}");
}
