// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for both sides of a call to a function owning a resource parameter:
//! the callee binds the parameter on entry to a local of its own, and the
//! caller hands its binding over by `move`, ending it without a release.

use miri::ast::expression::ExpressionKind;
use miri::ast::statement::StatementKind as AstStatementKind;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::mir::drop_elaboration::elaborate_drops;
use miri::mir::lowering::{lower_class_method, lower_function};
use miri::mir::{
    ArgMode, Body, EndCause, Local, Operand, Place, Rvalue, StatementKind, TerminatorKind,
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

fn cond(h Handle, c bool) int
    if c
        let k = close(h)
        println("c")
    println("after")
    return 1

fn peek(h Handle) int
    return h.id
"#;

/// The lowered body of the function `name` the fixture declares.
fn lowered(program: &str, name: &str) -> Body {
    lowered_from(&format!("{HANDLE}\n{program}"), name)
}

/// The lowered body of the function `name` that `source` declares.
fn lowered_from(source: &str, name: &str) -> Body {
    let result = Pipeline::new()
        .frontend(source)
        .expect("the fixture type checks");
    let function = (result.ast.body.iter())
        .find(
            |stmt| matches!(&stmt.node, AstStatementKind::FunctionDeclaration(f) if f.name == name),
        )
        .expect("the fixture declares the function");
    lower_function(function, &result.type_checker, false, false)
        .expect("the fixture lowers")
        .body
}

const MAIN: &str = r#"
fn main()
    let h = Handle(1)
    let n = cond(h, true)
    println(f"{n}")
"#;

/// The local past the parameters bound to the name `name`.
fn owned_local(body: &Body, name: &str) -> Local {
    let index = (body.local_decls.iter().enumerate())
        .position(|(index, decl)| index > body.arg_count && decl.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no local `{name}` past the parameters in\n{body}"));
    Local(index)
}

#[test]
fn a_kept_resource_parameter_is_on_the_signature_and_moved_into_a_local_on_entry() {
    let body = lowered(MAIN, "cond");
    assert_eq!(body.signature.kept_params, vec![true, false], "{body}");
    let h = owned_local(&body, "h");
    let moves_parameter = body.basic_blocks[0].statements.iter().any(|statement| {
        statement.kind
            == StatementKind::Assign(
                Place::new(h),
                Rvalue::Use(Operand::Move(Place::new(Local(1)))),
            )
    });
    assert!(
        moves_parameter,
        "the entry block binds `h` by move:\n{body}"
    );
}

#[test]
fn a_parameter_the_callee_only_reads_is_not_kept() {
    let body = lowered(MAIN, "peek");
    assert!(body.signature.kept_params.is_empty(), "{body}");
}

#[test]
fn a_function_named_as_a_value_keeps_its_parameter_at_direct_calls() {
    let program = format!("{MAIN}\nfn other()\n    let f = cond\n");
    let body = lowered(&program, "cond");
    assert_eq!(body.signature.kept_params, vec![true, false], "{body}");
}

/// The call in `body` naming the function whose link name ends in `name`.
fn call_to<'b>(body: &'b Body, name: &str) -> (&'b [Operand], &'b [ArgMode]) {
    (body.basic_blocks.iter())
        .filter_map(|block| block.terminator.as_ref())
        .find_map(|terminator| match &terminator.kind {
            TerminatorKind::Call {
                func,
                args,
                arg_modes,
                ..
            } if func
                .called_symbol()
                .is_some_and(|symbol| symbol.link_name().ends_with(name)) =>
            {
                Some((args.as_slice(), arg_modes.as_slice()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no call to `{name}` in\n{body}"))
}

fn ends_by(body: &Body, local: Local, cause: EndCause) -> usize {
    (body.basic_blocks.iter())
        .flat_map(|block| &block.statements)
        .filter(|statement| {
            matches!(&statement.kind, StatementKind::StorageDead(place, found)
                if place.local == local && place.projection.is_empty() && *found == cause)
        })
        .count()
}

#[test]
fn a_consumed_binding_is_handed_over_by_move_and_ended_without_a_release() {
    let body = lowered(MAIN, "main");
    let h = owned_local(&body, "h");
    let (args, modes) = call_to(&body, ".cond");
    assert_eq!(args[0], Operand::Move(Place::new(h)), "{body}");
    assert_eq!(modes[0], ArgMode::Owned, "{body}");
    assert_eq!(modes[1], ArgMode::Borrowed, "{body}");
    assert_eq!(ends_by(&body, h, EndCause::Moved), 1, "{body}");
    assert_eq!(ends_by(&body, h, EndCause::Consumed), 0, "{body}");
}

#[test]
fn a_temporary_argument_is_handed_over_by_move_and_ended_without_a_release() {
    let body = lowered("fn main()\n    let n = cond(Handle(1), true)\n", "main");
    let (args, modes) = call_to(&body, ".cond");
    let Operand::Move(temporary) = &args[0] else {
        panic!("the temporary is moved:\n{body}");
    };
    assert_eq!(modes[0], ArgMode::Owned, "{body}");
    assert_eq!(
        ends_by(&body, temporary.local, EndCause::Moved),
        1,
        "{body}"
    );
    assert_eq!(
        ends_by(&body, temporary.local, EndCause::Scope),
        0,
        "{body}"
    );
}

#[test]
fn a_field_argument_is_copied_for_the_callee_to_be_funded() {
    let program = r#"
class Holder
    public var h Handle
    fn init(h Handle)
        self.h = h

fn main()
    let holder = Holder(Handle(5))
    let n = cond(holder.h, true)
"#;
    let body = lowered(program, "main");
    let (args, modes) = call_to(&body, ".cond");
    assert!(matches!(args[0], Operand::Copy(_)), "{body}");
    assert_eq!(modes[0], ArgMode::Owned, "{body}");
}

#[test]
fn a_parameter_kept_in_every_arm_needs_no_drop_flag() {
    let program = r#"
fn both(h Handle, c bool) int
    if c
        let k = close(h)
    else
        let k = close(h)
    return 1
"#;
    let mut body = lowered(program, "both");
    elaborate_drops(&mut body).expect("the fixture elaborates");
    let flags = (body.local_decls.iter()).filter(|decl| decl.drop_flag_of().is_some());
    assert_eq!(flags.count(), 0, "a flag was declared:\n{body}");
    let h = owned_local(&body, "h");
    assert_eq!(ends_by(&body, h, EndCause::Moved), 2, "{body}");
    assert_eq!(ends_by(&body, h, EndCause::Scope), 0, "{body}");
}

#[test]
fn a_parameter_kept_in_one_arm_is_released_behind_its_drop_flag() {
    let mut body = lowered(MAIN, "cond");
    elaborate_drops(&mut body).expect("the fixture elaborates");
    let h = owned_local(&body, "h");
    let guarded: Vec<Local> = (body.local_decls.iter())
        .filter_map(|decl| decl.drop_flag_of())
        .collect();
    assert_eq!(guarded, vec![h], "one flag, guarding `h`:\n{body}");
}

/// The owned local is released by reference counting where it ends: reading
/// it as a borrow of the parameter would drop that release.
#[test]
fn the_local_a_kept_parameter_moves_into_is_never_a_borrow() {
    let source = format!("{HANDLE}\n{MAIN}");
    let (bodies, facts) = Pipeline::new()
        .get_mir_program_with_rc(&source)
        .expect("the fixture compiles");
    let (index, (_, body)) = (bodies.iter().enumerate())
        .find(|(_, (name, _))| name.ends_with(".cond"))
        .expect("the program has `cond`");
    let ownership = facts
        .iter()
        .nth(index)
        .expect("facts for `cond`")
        .ownership();
    let h = owned_local(body, "h");
    assert!(!ownership.is_borrowed(h), "`h` reads as a borrow:\n{body}");
    assert!(ownership.is_owned_param(Local(1)), "{body}");
    let releases = (body.basic_blocks.iter())
        .flat_map(|block| &block.statements)
        .filter(|statement| statement.kind == StatementKind::DecRef(Place::new(h)))
        .count();
    assert_eq!(releases, 1, "`h` is released where its scope ends:\n{body}");
}

#[test]
fn an_element_argument_is_copied_for_the_callee_to_be_funded() {
    let source = format!(
        "use system.collections.list\n{HANDLE}\nfn main()\n    let xs = List([Handle(1)])\n    let n = cond(xs[0], true)\n"
    );
    let body = lowered_from(&source, "main");
    let (args, modes) = call_to(&body, ".cond");
    assert!(matches!(args[0], Operand::Copy(_)), "{body}");
    assert_eq!(modes[0], ArgMode::Owned, "{body}");
}

const SHELF: &str = r#"
struct Shelf
    slot Handle?

class Rack
    fn init()
        println("rack")
    fn put(h Handle) int
        let _shelf = Shelf(slot: Some(h))
        return 0

class Keeper
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed keeper {self.id}")
    fn put_on(self, n int) int
        if true
            let _shelf = KeeperShelf(slot: Some(self))
        return n

struct KeeperShelf
    slot Keeper?

class Point
    public var x int
    fn init(x int)
        self.x = x
    fn moved(self) Point
        return self

struct Holder
    k Keeper
"#;

/// The method `method` of the type `owner` the fixture declares, lowered
/// before any MIR pass runs.
fn lowered_method(program: &str, owner: &str, method: &str) -> Body {
    let source = format!("{HANDLE}\n{SHELF}\n{program}");
    let result = Pipeline::new()
        .frontend(&source)
        .expect("the fixture type checks");
    let members = (result.ast.body.iter())
        .find_map(|stmt| match &stmt.node {
            AstStatementKind::Class(data) => matches!(
                &data.name.node,
                ExpressionKind::Identifier(name, _) if name == owner
            )
            .then_some(data.body.as_slice()),
            _ => None,
        })
        .expect("the fixture declares the type");
    let statement = (members.iter())
        .find(|member| {
            matches!(&member.node, AstStatementKind::FunctionDeclaration(f) if f.name == method)
        })
        .expect("the type declares the method");
    let receiver = Type::new(TypeKind::Custom(owner.to_string(), None), Span::default());
    lower_class_method(statement, receiver, &result.type_checker, false)
        .expect("the fixture lowers")
        .body
}

const KEEPER_MAIN: &str = r#"
fn main()
    let k = Keeper(1)
    let n = k.put_on(2)
    let holder = Holder(k: Keeper(2))
    let m = holder.k.put_on(3)
    let rack = Rack()
    let h = Handle(3)
    let r = rack.put(h)
"#;

#[test]
fn a_kept_receiver_is_on_the_signature_and_moved_into_a_local_on_entry() {
    let body = lowered_method(KEEPER_MAIN, "Keeper", "put_on");
    assert_eq!(body.signature.kept_params, vec![true, false], "{body}");
    assert_eq!(body.signature.receiver, Some(Local(1)), "{body}");
    let this = owned_local(&body, "self");
    let moves_receiver = body.basic_blocks[0].statements.iter().any(|statement| {
        statement.kind
            == StatementKind::Assign(
                Place::new(this),
                Rvalue::Use(Operand::Move(Place::new(Local(1)))),
            )
    });
    assert!(
        moves_receiver,
        "the entry block binds `self` by move:\n{body}"
    );
}

#[test]
fn a_kept_method_parameter_is_on_the_signature_after_the_receiver() {
    let body = lowered_method(KEEPER_MAIN, "Rack", "put");
    assert_eq!(body.signature.kept_params, vec![false, true], "{body}");
    let h = owned_local(&body, "h");
    let moves_parameter = body.basic_blocks[0].statements.iter().any(|statement| {
        statement.kind
            == StatementKind::Assign(
                Place::new(h),
                Rvalue::Use(Operand::Move(Place::new(Local(2)))),
            )
    });
    assert!(
        moves_parameter,
        "the entry block binds `h` by move:\n{body}"
    );
}

#[test]
fn a_receiver_that_is_no_resource_is_never_kept() {
    let body = lowered_method(KEEPER_MAIN, "Point", "moved");
    assert!(body.signature.kept_params.is_empty(), "{body}");
}

#[test]
fn a_consumed_receiver_is_handed_over_by_move_and_ended_without_a_release() {
    let source = format!("{HANDLE}\n{SHELF}\n{KEEPER_MAIN}");
    let body = lowered_from(&source, "main");
    let k = owned_local(&body, "k");
    let (args, modes) = call_to(&body, ".put_on");
    assert_eq!(args[0], Operand::Move(Place::new(k)), "{body}");
    assert_eq!(modes[0], ArgMode::Owned, "{body}");
    assert_eq!(modes[1], ArgMode::Borrowed, "{body}");
    assert_eq!(ends_by(&body, k, EndCause::Moved), 1, "{body}");
    assert_eq!(ends_by(&body, k, EndCause::Consumed), 0, "{body}");
}

#[test]
fn a_kept_receivers_caller_releases_nothing_it_handed_over() {
    let source = format!("{HANDLE}\n{SHELF}\n{KEEPER_MAIN}");
    let (bodies, _) = Pipeline::new()
        .get_mir_program_with_rc(&source)
        .expect("the fixture compiles");
    let (_, body) = (bodies.iter())
        .find(|(name, _)| name == "main" || name.ends_with(".main"))
        .expect("the program has `main`");
    let k = owned_local(body, "k");
    let releases = (body.basic_blocks.iter())
        .flat_map(|block| &block.statements)
        .filter(|statement| statement.kind == StatementKind::DecRef(Place::new(k)))
        .count();
    assert_eq!(releases, 0, "`k` is the method's from the call on:\n{body}");
}

#[test]
fn a_field_receiver_is_copied_for_the_method_to_be_funded() {
    let source = format!("{HANDLE}\n{SHELF}\n{KEEPER_MAIN}");
    let body = lowered_from(&source, "main");
    let holder = owned_local(&body, "holder");
    let field_call = (body.basic_blocks.iter())
        .filter_map(|block| block.terminator.as_ref())
        .find_map(|terminator| match &terminator.kind {
            TerminatorKind::Call {
                args, arg_modes, ..
            } if matches!(args.first(), Some(Operand::Copy(place))
                if place.local == holder && !place.projection.is_empty()) =>
            {
                Some((args.clone(), arg_modes.clone()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no call on `holder.k` in\n{body}"));
    assert_eq!(field_call.1[0], ArgMode::Owned, "{body}");
}

#[test]
fn a_consumed_method_argument_is_handed_over_by_move() {
    let source = format!("{HANDLE}\n{SHELF}\n{KEEPER_MAIN}");
    let body = lowered_from(&source, "main");
    let h = owned_local(&body, "h");
    let (args, modes) = call_to(&body, ".put");
    assert_eq!(modes[0], ArgMode::Borrowed, "the rack is lent:\n{body}");
    assert_eq!(args[1], Operand::Move(Place::new(h)), "{body}");
    assert_eq!(modes[1], ArgMode::Owned, "{body}");
    assert_eq!(ends_by(&body, h, EndCause::Moved), 1, "{body}");
}

/// The bodies of the closures the function `name` the fixture declares
/// builds: its lambdas, nested functions and function-value thunks.
fn lowered_closures(program: &str, name: &str) -> (Body, Vec<Body>) {
    let source = format!("{HANDLE}\n{program}");
    let result = Pipeline::new()
        .frontend(&source)
        .expect("the fixture type checks");
    let function = (result.ast.body.iter())
        .find(
            |stmt| matches!(&stmt.node, AstStatementKind::FunctionDeclaration(f) if f.name == name),
        )
        .expect("the fixture declares the function");
    let lowered =
        lower_function(function, &result.type_checker, false, false).expect("the fixture lowers");
    let closures = lowered.lambdas.into_iter().map(|info| info.body).collect();
    (lowered.body, closures)
}

/// The one call in `body` through a closure value, rather than a named
/// function.
fn closure_call(body: &Body) -> (&[Operand], &[ArgMode]) {
    (body.basic_blocks.iter())
        .filter_map(|block| block.terminator.as_ref())
        .find_map(|terminator| match &terminator.kind {
            TerminatorKind::Call {
                func: Operand::Copy(_) | Operand::Move(_),
                args,
                arg_modes,
                ..
            } => Some((args.as_slice(), arg_modes.as_slice())),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no call through a closure in\n{body}"))
}

const VALUE_MAIN: &str = r#"
fn main()
    let f = cond
    let h = Handle(1)
    let n = f(h, true)
    let g = peek
    let m = g(Handle(2))
    println(f"{n} {m}")
"#;

#[test]
fn a_function_value_thunk_owns_every_resource_parameter() {
    let (_, closures) = lowered_closures(VALUE_MAIN, "main");
    assert_eq!(closures.len(), 2, "one thunk per function named as a value");
    for thunk in &closures {
        assert_eq!(
            thunk.signature.kept_params.get(..2),
            Some(&[false, true][..]),
            "the thunk owns the handle, after its environment:\n{thunk}"
        );
    }
}

#[test]
fn a_thunk_hands_its_target_exactly_what_the_target_keeps() {
    let (_, closures) = lowered_closures(VALUE_MAIN, "main");
    let keeper_thunk = (closures.iter())
        .find(|body| {
            (body.basic_blocks.iter())
                .filter_map(|block| block.terminator.as_ref())
                .any(|terminator| {
                    matches!(&terminator.kind, TerminatorKind::Call { func, .. }
                    if func.called_symbol().is_some_and(|s| s.link_name().ends_with(".cond")))
                })
        })
        .expect("a thunk forwarding to `cond`");
    let (args, modes) = call_to(keeper_thunk, ".cond");
    assert!(matches!(args[0], Operand::Move(_)), "{keeper_thunk}");
    assert_eq!(modes[0], ArgMode::Owned, "{keeper_thunk}");
    assert_eq!(modes[1], ArgMode::Borrowed, "{keeper_thunk}");
    let reader_thunk = (closures.iter())
        .find(|body| !std::ptr::eq(*body, keeper_thunk))
        .expect("a thunk forwarding to `peek`");
    let (_, modes) = call_to(reader_thunk, ".peek");
    assert_eq!(modes[0], ArgMode::Borrowed, "{reader_thunk}");
}

#[test]
fn a_call_through_a_function_value_hands_over_each_resource_argument() {
    let (body, _) = lowered_closures(VALUE_MAIN, "main");
    let h = owned_local(&body, "h");
    let (args, modes) = closure_call(&body);
    assert_eq!(args[0], Operand::Move(Place::new(h)), "{body}");
    assert_eq!(modes[0], ArgMode::Owned, "{body}");
    assert_eq!(modes[1], ArgMode::Borrowed, "{body}");
    assert_eq!(ends_by(&body, h, EndCause::Moved), 1, "{body}");
}

#[test]
fn a_lambda_owns_every_resource_parameter_and_binds_it_on_entry() {
    let program = r#"
fn main()
    let f = fn(h Handle, n int) int
        return n
    let m = f(Handle(1), 2)
    println(f"{m}")
"#;
    let (_, closures) = lowered_closures(program, "main");
    let [lambda] = closures.as_slice() else {
        panic!("one lambda body");
    };
    assert_eq!(
        lambda.signature.kept_params,
        vec![false, true, false],
        "{lambda}"
    );
    let h = owned_local(lambda, "h");
    let moves_parameter = lambda.basic_blocks[0].statements.iter().any(|statement| {
        statement.kind
            == StatementKind::Assign(
                Place::new(h),
                Rvalue::Use(Operand::Move(Place::new(Local(2)))),
            )
    });
    assert!(
        moves_parameter,
        "the entry block binds `h` by move:\n{lambda}"
    );
}
