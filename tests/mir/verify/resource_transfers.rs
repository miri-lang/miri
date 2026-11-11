// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the rule that a resource a body owns is handed over by `move`,
//! never by a copy funded with a retain, and the shared fixture the verifier
//! mutation tests of a kept resource parameter start from: the bodies the
//! passes leave for a program whose `cond` owns the handle it keeps in one
//! arm.

use miri::mir::facts::{OwnershipFacts, ProgramFacts};
use miri::mir::type_facts::DropHookReach;
use miri::mir::verify::verify_resource_transfers;
use miri::mir::{Body, Local, Operand, Place, Statement, StatementKind, TerminatorKind};
use miri::pipeline::{Pipeline, PipelineResult};

pub(super) const KEEPER: &str = r#"
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

fn main()
    let h = Handle(1)
    let n = cond(h, true)
    println(f"{n}")
"#;

/// The bodies the MIR passes leave for [`KEEPER`], with their facts and the
/// checked program they were compiled from.
pub(super) struct Compiled {
    pub bodies: Vec<(String, Body)>,
    pub facts: ProgramFacts,
    pub checked: PipelineResult,
}

impl Compiled {
    pub(super) fn of(source: &str) -> Self {
        let pipeline = Pipeline::new();
        let (bodies, facts) = pipeline
            .get_mir_program_with_rc(source)
            .expect("the fixture compiles");
        let checked = pipeline
            .frontend_script(source)
            .expect("the fixture type checks");
        Compiled {
            bodies,
            facts,
            checked,
        }
    }

    /// The body whose name ends in `.{name}`, or is `name`, with its
    /// ownership facts.
    pub(super) fn body(&self, name: &str) -> (Body, OwnershipFacts) {
        let suffix = format!(".{name}");
        let (index, (_, body)) = (self.bodies.iter().enumerate())
            .find(|(_, (found, _))| found.ends_with(&suffix) || found == name)
            .unwrap_or_else(|| panic!("no body `{name}`"));
        let facts = self.facts.iter().nth(index).expect("facts for the body");
        (body.clone(), facts.ownership().clone())
    }

    pub(super) fn hooks(&self) -> DropHookReach<'_> {
        DropHookReach::new(self.checked.type_checker.type_definitions())
    }
}

/// The local past the parameters of `body` bound to the name `name`.
pub(super) fn owned_local(body: &Body, name: &str) -> Local {
    let index = (body.local_decls.iter().enumerate())
        .position(|(index, decl)| index > body.arg_count && decl.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no local `{name}` past the parameters in\n{body}"));
    Local(index)
}

/// Rewrites the `move` of `local` a call in `body` hands over into a copy
/// with a retain before the call, the way a funded argument is spelled.
fn fund_instead_of_moving(body: &mut Body, local: Local) {
    for block in &mut body.basic_blocks {
        let Some(TerminatorKind::Call { args, .. }) = block
            .terminator
            .as_mut()
            .map(|terminator| &mut terminator.kind)
        else {
            continue;
        };
        let Some(arg) = (args.iter_mut()).find(|arg| **arg == Operand::Move(Place::new(local)))
        else {
            continue;
        };
        *arg = Operand::Copy(Place::new(local));
        block.statements.push(Statement {
            kind: StatementKind::IncRef(Place::new(local)),
            span: Default::default(),
        });
        return;
    }
    panic!("no call moves the local in\n{body}");
}

#[test]
fn a_resource_handed_over_by_move_is_accepted() {
    let compiled = Compiled::of(KEEPER);
    for name in ["cond", "main"] {
        let (body, ownership) = compiled.body(name);
        let found = verify_resource_transfers(&body, &ownership, &compiled.hooks());
        assert!(found.is_empty(), "{name}: {found:?}\n{body}");
    }
}

#[test]
fn a_resource_the_caller_hands_over_by_a_funded_copy_is_reported() {
    let compiled = Compiled::of(KEEPER);
    let (mut body, ownership) = compiled.body("main");
    let h = owned_local(&body, "h");
    fund_instead_of_moving(&mut body, h);
    let found = verify_resource_transfers(&body, &ownership, &compiled.hooks());
    assert_eq!(found.len(), 1, "{found:?}\n{body}");
    assert_eq!(found[0].local, h);
}

#[test]
fn a_kept_parameter_the_callee_hands_on_by_a_funded_copy_is_reported() {
    let compiled = Compiled::of(KEEPER);
    let (mut body, ownership) = compiled.body("cond");
    let h = owned_local(&body, "h");
    fund_instead_of_moving(&mut body, h);
    let found = verify_resource_transfers(&body, &ownership, &compiled.hooks());
    assert_eq!(found.len(), 1, "{found:?}\n{body}");
    assert_eq!(found[0].local, h);
}
