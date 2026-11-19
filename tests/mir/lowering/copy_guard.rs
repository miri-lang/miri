// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering judges again every copy the reached program contains: a copy of
//! a resource with no `clone` that reached lowering unrefused is a compiler
//! defect, reported as one rather than compiled into a resource with two
//! holders. Each test skips the type checker's refusal, as a defect in the
//! judgement would, and hands lowering what the checker reached.

use miri::ast::statement::StatementKind;
use miri::ast::types::{Type, TypeKind};
use miri::error::syntax::Span;
use miri::lexer::Lexer;
use miri::mir::lowering::copy_guard::{reached_copy_symbol, refuse_unjudged_copies};
use miri::mir::lowering::{lower_generic_instantiation, Lowered};
use miri::mir::symbol::Symbol;
use miri::mir::{Body, Operand, PlaceElem, TerminatorKind};
use miri::parser::Parser;
use miri::type_checker::{ModuleId, TypeChecker};
use std::collections::HashMap;

/// `dup` copies a list of its own parameter through the list constructor,
/// which calls the runtime's copy in `dup`'s own body, and `main` reaches it
/// at a resource with no `clone` and at `int`.
const DUP: &str = r#"
use system.collections.list

class Res
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"drop {self.id}")

fn dup<T>(l List<T>) List<T>
    return List(l)

fn main()
    var l = List<Res>()
    l.push(Res(1))
    let d = dup(l)
    let n = dup(List([1]))
"#;

/// The checker that judged `DUP`, any refusal skipped, and the program.
fn checked_ignoring_refusals() -> (TypeChecker, miri::ast::Program) {
    let mut lexer = Lexer::new(DUP);
    let mut parser = Parser::new(&mut lexer, DUP);
    let mut ast = parser.parse_all().expect("the program parses");
    miri::ast::normalize::normalize(&mut ast);
    let mut checker = TypeChecker::new();
    // Whatever the checker judged, lowering is handed every copy it reached.
    let _judged = checker.check(&mut ast);
    (checker, ast)
}

/// `dup` lowered at `T = argument`, under the symbol of its reached copy at
/// that argument.
fn lowered_dup(checker: &TypeChecker, ast: &miri::ast::Program, argument: &str) -> (Symbol, Body) {
    let dup = (ast.body.iter())
        .find(|stmt| matches!(&stmt.node, StatementKind::FunctionDeclaration(f) if f.name == "dup"))
        .expect("`dup` is declared");
    let program = checker.reached_program();
    let copy = (program.copies().iter())
        .find(|copy| {
            copy.name == "dup"
                && matches!(&copy.arguments[..], [ty] if ty.kind.to_string() == argument)
        })
        .expect("the program reaches the copy");
    let ty = copy.arguments[0].clone();
    let subs = HashMap::from([("T".to_string(), ty)]);
    let Lowered { body, .. } =
        lower_generic_instantiation(dup, checker, false, false, &subs).expect("the copy lowers");
    (reached_copy_symbol(copy), body)
}

#[test]
fn a_reached_copy_of_a_resource_with_no_clone_is_refused_by_lowering() {
    let (checker, ast) = checked_ignoring_refusals();
    let lowered = [lowered_dup(&checker, &ast, "Res")];
    let error = refuse_unjudged_copies(&checker, &lowered).expect_err("lowering refuses the copy");
    let message = error.to_string();
    assert!(
        message.contains("copies `Res` through `miri_rt_list_clone`, a resource with no `clone`"),
        "the error names the element: {message}"
    );
}

#[test]
fn a_reached_copy_of_a_plain_value_is_accepted_by_lowering() {
    let (checker, ast) = checked_ignoring_refusals();
    let lowered = [lowered_dup(&checker, &ast, "int")];
    assert!(refuse_unjudged_copies(&checker, &lowered).is_ok());
}

#[test]
fn a_body_the_program_does_not_reach_is_not_judged_by_lowering() {
    let (checker, ast) = checked_ignoring_refusals();
    let (_, body) = lowered_dup(&checker, &ast, "Res");
    let unreached = Symbol::function(
        &ModuleId::Program,
        "unreached",
        &[Type::new(TypeKind::Int, Span::default())],
    );
    assert!(refuse_unjudged_copies(&checker, &[(unreached, body)]).is_ok());
}

/// A copy whose copied argument is a place whose type the body's
/// declarations give nowhere cannot be judged: lowering refuses it rather than
/// letting it through unjudged.
#[test]
fn a_reached_copy_of_an_argument_of_unstated_type_is_refused_by_lowering() {
    let (checker, ast) = checked_ignoring_refusals();
    let (symbol, mut body) = lowered_dup(&checker, &ast, "int");
    let copying_call = (body.basic_blocks.iter_mut())
        .filter_map(|block| block.terminator.as_mut())
        .find_map(|terminator| match &mut terminator.kind {
            TerminatorKind::Call { func, args, .. } if func.called_runtime_effect().is_some() => {
                Some(args)
            }
            _ => None,
        })
        .expect("`dup` calls the runtime's copy");
    let Some(Operand::Copy(place) | Operand::Move(place)) = copying_call.first_mut() else {
        panic!("the copy is handed a place");
    };
    // No field of the list sits at this index, so no type is declared there.
    place.projection.push(PlaceElem::Field(usize::MAX));
    let error = refuse_unjudged_copies(&checker, &[(symbol, body)])
        .expect_err("lowering refuses a copy it cannot judge");
    assert!(
        error.to_string().contains("whose type it does not state"),
        "the error says why: {error}"
    );
}
