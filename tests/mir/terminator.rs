// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::mir::body::DeviceHandleId;
use miri::mir::operand::Operand;
use miri::mir::place::{Local, Place};
use miri::mir::terminator::{validate_call_arg_handles, GpuLaunchArgs};

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
    assert_eq!(args.arg_handles().len(), 2);
}

#[test]
fn gpu_launch_args_accepts_no_captures() {
    let args =
        GpuLaunchArgs::new(vec![], vec![], vec![], vec![]).expect("empty launch must construct");
    assert!(args.is_empty());
    assert_eq!(args.len(), 0);
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
    assert!(validate_call_arg_handles(&args, &handles).is_ok());
}

#[test]
fn call_arg_handles_accepts_equal_length() {
    // Non-empty arg_handles matching args.len() is valid.
    let handles = vec![None, None];
    let args = vec![dummy_operand(), dummy_operand()];
    assert!(validate_call_arg_handles(&args, &handles).is_ok());
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
