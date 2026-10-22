// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::error::syntax::Span;
use miri::mir::{
    ArgMode, BasicBlock, BasicBlockData, Body, ExecutionModel, Local, Operand, Place, Rvalue,
    Statement, StatementKind, Terminator, TerminatorKind,
};

fn statement(kind: StatementKind) -> Statement {
    Statement {
        kind,
        span: Span::default(),
    }
}

#[test]
fn test_written_place_is_an_assignment_destination_only() {
    let target = Place::new(Local(1));
    let copied = Rvalue::Use(Operand::Copy(Place::new(Local(2))));
    for write in [
        StatementKind::Assign(target.clone(), copied.clone()),
        StatementKind::Reassign(target.clone(), copied),
    ] {
        assert_eq!(statement(write).written_place(), Some(&target));
    }
    for other in [
        StatementKind::IncRef(target.clone()),
        StatementKind::DecRef(target.clone()),
        StatementKind::StorageDead(target),
        StatementKind::Nop,
    ] {
        assert_eq!(statement(other).written_place(), None);
    }
}

#[test]
fn test_written_locals_counts_statement_and_terminator_writes() {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    let mut block = BasicBlockData::new(Some(Terminator::new(
        TerminatorKind::Call {
            func: Operand::runtime("callee", Span::default()),
            args: vec![
                Operand::Copy(Place::new(Local(3))),
                Operand::Copy(Place::new(Local(4))),
            ],
            arg_modes: vec![ArgMode::Borrowed, ArgMode::Out],
            arg_handles: Vec::new(),
            destination: Place::new(Local(2)),
            target: Some(BasicBlock(0)),
        },
        Span::default(),
    )));
    block.statements.push(statement(StatementKind::Assign(
        Place::new(Local(1)),
        Rvalue::Use(Operand::Copy(Place::new(Local(5)))),
    )));
    body.basic_blocks.push(block);
    let written = body.written_locals();
    let mut locals: Vec<usize> = written.iter().map(|local| local.0).collect();
    locals.sort_unstable();
    assert_eq!(locals, vec![1, 2, 4]);
}

/// A kernel lowered out of a `gpu forall` inside an imported module's
/// function records the module's file, as the function holding it does.
#[test]
fn test_a_kernel_lowered_in_a_module_records_the_module_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("kernels.mi"),
        "use system.gpu\nuse system.collections.array\n\npublic fn fill()\n    gpu var dst = [0, 0, 0, 0]\n    gpu forall i in 0..4\n        dst[i] = i\n",
    )
    .unwrap();
    let bodies = miri::pipeline::Pipeline::new()
        .with_source_dir(dir.path().to_path_buf())
        .get_gpu_mir_bodies("use local.kernels\n\nfn main()\n    fill()\n")
        .unwrap();
    let names: Vec<&str> = bodies.iter().map(|(name, _)| name.as_str()).collect();
    let source_of = |wanted: &dyn Fn(&str, &Body) -> bool| {
        bodies
            .iter()
            .find(|(name, body)| wanted(name, body))
            .map(|(_, body)| body.source)
            .unwrap_or_else(|| panic!("no such body among {names:?}"))
    };
    let fill = source_of(&|name, _| name.ends_with(".fill"));
    let kernel = source_of(&|_, body| body.execution_model == ExecutionModel::GpuKernel);
    let main = source_of(&|name, _| name == "main");
    assert_eq!(main, Some(miri::error::SourceId::PROGRAM));
    assert!(fill.is_some() && fill != main, "fill records {fill:?}");
    assert_eq!(kernel, fill);
}
