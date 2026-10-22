// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::error::syntax::Span;
use miri::mir::body::DeviceHandleId;
use miri::mir::operand::Operand;
use miri::mir::place::{Local, Place};
use miri::mir::terminator::{validate_call_arg_handles, GpuLaunchArgs};
use miri::mir::{ArgMode, BasicBlock, Terminator, TerminatorKind};

fn dummy_operand() -> Operand {
    Operand::Copy(Place::new(Local(0)))
}

#[test]
fn gpu_launch_args_accepts_equal_lengths() {
    let built = GpuLaunchArgs::new(
        vec![dummy_operand(), dummy_operand()],
        vec![None, None],
        vec![true, false],
        vec![false, true],
    );
    let args = built.expect("equal-length vectors must construct");
    assert_eq!(args.len(), 2);
    assert!(!args.is_empty());
    assert_eq!(args.arg_read_only(), &[true, false]);
    assert_eq!(args.arg_int_narrow(), &[false, true]);
    assert_eq!(args.arg_handles(), &[None, None]);
    assert_eq!(args.args().len(), 2);
}

#[test]
fn gpu_launch_args_accepts_no_captures() {
    let args =
        GpuLaunchArgs::new(vec![], vec![], vec![], vec![]).expect("empty launch must construct");
    assert!(args.is_empty());
    assert_eq!(args.len(), 0);
    assert!(args.args().is_empty());
    assert_eq!(args.arg_handles(), &[] as &[Option<DeviceHandleId>]);
    assert_eq!(args.arg_read_only(), &[] as &[bool]);
    assert_eq!(args.arg_int_narrow(), &[] as &[bool]);
}

#[test]
fn gpu_launch_args_rejects_short_handles() {
    let err = GpuLaunchArgs::new(
        vec![dummy_operand(), dummy_operand()],
        vec![None],
        vec![true, false],
        vec![false, false],
    )
    .expect_err("mismatched arg_handles must be rejected");
    assert_eq!(err.field, "arg_handles");
    assert_eq!(err.expected, 2);
    assert_eq!(err.got, 1);
}

#[test]
fn gpu_launch_args_rejects_long_read_only() {
    let err = GpuLaunchArgs::new(
        vec![dummy_operand()],
        vec![None],
        vec![true, false],
        vec![false],
    )
    .expect_err("mismatched arg_read_only must be rejected");
    assert_eq!(err.field, "arg_read_only");
}

#[test]
fn gpu_launch_args_rejects_mismatched_int_narrow() {
    let err = GpuLaunchArgs::new(vec![dummy_operand()], vec![None], vec![true], vec![])
        .expect_err("mismatched arg_int_narrow must be rejected");
    assert_eq!(err.field, "arg_int_narrow");
    assert_eq!(err.got, 0);
}

#[test]
fn gpu_launch_args_mut_preserves_length() {
    let mut args = GpuLaunchArgs::new(vec![dummy_operand()], vec![None], vec![true], vec![false])
        .expect("constructs");
    args.args_mut()[0] = dummy_operand();
    assert_eq!(args.len(), 1);
    assert_eq!(args.arg_read_only().len(), args.args().len());
}

#[test]
fn call_arg_handles_accepts_empty() {
    // Empty arg_handles is always valid: an ordinary host call carries no
    // device-handle metadata.
    let handles: Vec<Option<DeviceHandleId>> = Vec::new();
    let args = vec![dummy_operand()];
    assert_eq!(validate_call_arg_handles(&args, &handles), Ok(()));
}

#[test]
fn call_arg_handles_accepts_equal_length() {
    // Non-empty arg_handles matching args.len() is valid.
    let handles = vec![None, None];
    let args = vec![dummy_operand(), dummy_operand()];
    assert_eq!(validate_call_arg_handles(&args, &handles), Ok(()));
}

#[test]
fn call_arg_handles_rejects_short() {
    // Non-empty arg_handles shorter than args is invalid.
    let handles = vec![None];
    let args = vec![dummy_operand(), dummy_operand()];
    let err = validate_call_arg_handles(&args, &handles)
        .expect_err("mismatched arg_handles must be rejected");
    assert_eq!(err.expected, 2);
    assert_eq!(err.got, 1);
}

#[test]
fn call_arg_handles_rejects_long() {
    // Non-empty arg_handles longer than args is invalid.
    let handles = vec![None, None, None];
    let args = vec![dummy_operand()];
    let err = validate_call_arg_handles(&args, &handles)
        .expect_err("mismatched arg_handles must be rejected");
    assert_eq!(err.expected, 1);
    assert_eq!(err.got, 3);
}

fn call(args: Vec<Operand>, arg_modes: Vec<ArgMode>) -> TerminatorKind {
    TerminatorKind::Call {
        func: Operand::runtime("callee", Span::default()),
        args,
        arg_modes,
        arg_handles: Vec::new(),
        destination: Place::new(Local(9)),
        target: Some(BasicBlock(1)),
    }
}

fn written(kind: TerminatorKind) -> Vec<Place> {
    Terminator::new(kind, Span::default())
        .written_places()
        .cloned()
        .collect()
}

#[test]
fn written_places_names_a_call_destination_and_its_out_arguments() {
    let out = Place::new(Local(2));
    let kind = call(
        vec![
            Operand::Copy(Place::new(Local(1))),
            Operand::Copy(out.clone()),
        ],
        vec![ArgMode::Borrowed, ArgMode::Out],
    );
    assert_eq!(written(kind), vec![Place::new(Local(9)), out]);
}

#[test]
fn written_places_names_virtual_call_destinations() {
    let virtual_call = TerminatorKind::VirtualCall {
        vtable_slot: 0,
        args: vec![Operand::Copy(Place::new(Local(1)))],
        arg_modes: vec![ArgMode::Out],
        param_types: Vec::new(),
        destination: Place::new(Local(4)),
        target: None,
    };
    assert_eq!(
        written(virtual_call),
        vec![Place::new(Local(4)), Place::new(Local(1))]
    );
}

#[test]
fn written_places_names_launch_destinations() {
    let launch = TerminatorKind::GpuLaunch {
        kernel: dummy_operand(),
        grid: dummy_operand(),
        block: dummy_operand(),
        launch_args: GpuLaunchArgs::new(Vec::new(), Vec::new(), Vec::new(), Vec::new())
            .expect("no captures"),
        scalar_args: Vec::new(),
        uniform_bound_x: None,
        uniform_bound_y: None,
        uniform_bound_z: None,
        uniform_start_x: None,
        uniform_start_y: None,
        uniform_start_z: None,
        destination: Place::new(Local(5)),
        target: None,
    };
    assert_eq!(written(launch), vec![Place::new(Local(5))]);
}

#[test]
fn written_places_of_a_jump_or_a_return_is_empty() {
    assert!(written(TerminatorKind::Return).is_empty());
    assert!(written(TerminatorKind::Goto {
        target: BasicBlock(0)
    })
    .is_empty());
}
