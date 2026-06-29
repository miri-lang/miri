// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::mir::utils::mir_lower_code;
use miri::ast::statement::StatementKind;
use miri::mir::lowering::lower_function;
use miri::mir::{ExecutionModel, StorageClass, TerminatorKind};
use miri::pipeline::Pipeline;

#[test]
fn test_forall_gpu_emits_gpu_launch_terminator() {
    let body = mir_lower_code(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [5, 6, 7, 8]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = a[i] + b[i]
",
    );
    let has_launch = body.basic_blocks.iter().any(|bb| {
        bb.terminator
            .as_ref()
            .is_some_and(|t| matches!(t.kind, TerminatorKind::GpuLaunch { .. }))
    });
    assert!(
        has_launch,
        "Expected TerminatorKind::GpuLaunch in MIR for `gpu forall` body"
    );
}

fn synthesize_kernel_names(source: &str) -> Vec<String> {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend(source).expect("frontend");
    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|s| matches!(s.node, StatementKind::FunctionDeclaration(_)))
        .expect("a function declaration");
    let (_body, lambdas) =
        lower_function(func_stmt, &result.type_checker, false, false).expect("lowering");
    lambdas
        .into_iter()
        .filter(|l| l.body.execution_model == ExecutionModel::GpuKernel)
        .map(|l| l.name)
        .collect()
}

#[test]
fn test_two_forall_gpu_loops_in_one_function_have_unique_kernel_names() {
    let names = synthesize_kernel_names(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var a = [0, 0, 0, 0]
    gpu var b = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i in 0..4
        a[i] = i
    gpu forall j in 0..8
        b[j] = j
",
    );
    let mut sorted = names.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        names.len(),
        "expected unique kernel names, got {names:?}"
    );
}

#[test]
fn test_forall_gpu_captures_variables_used_inside_nested_range() {
    // The capture collector must walk into ExpressionKind::Range so that
    // variables used only as inner-loop bounds (`for j in 0..n`) are still
    // counted as outer captures. Before the fix this silently dropped `n`
    // from the capture list, leaving the kernel referencing an undefined
    // local.
    let pipeline = Pipeline::new();
    let source = "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        for j in 0..i
            dst[i] = a[i]
";
    let result = pipeline.frontend(source).expect("frontend");
    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|s| matches!(s.node, StatementKind::FunctionDeclaration(_)))
        .unwrap();
    let (_body, lambdas) =
        lower_function(func_stmt, &result.type_checker, false, false).expect("lowering");
    let kernel = lambdas
        .iter()
        .find(|l| l.body.execution_model == ExecutionModel::GpuKernel)
        .expect("kernel");
    let captured_names: Vec<&str> = kernel
        .body
        .local_decls
        .iter()
        .skip(1) // _0 is the return slot
        .take(kernel.body.arg_count)
        .filter_map(|d| d.name.as_deref())
        .collect();
    assert!(
        captured_names.contains(&"a"),
        "expected 'a' captured into kernel, got {captured_names:?}"
    );
    assert!(
        captured_names.contains(&"dst"),
        "expected 'dst' captured into kernel, got {captured_names:?}"
    );
}

#[test]
fn test_forall_gpu_2d_with_literal_bounds_has_none_uniform_bounds() {
    let body = mir_lower_code(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i, j in 0..3, 0..3
        dst[i * 3 + j] = i + j
",
    );
    let launch = body
        .basic_blocks
        .iter()
        .find_map(|bb| match bb.terminator.as_ref().map(|t| &t.kind) {
            Some(TerminatorKind::GpuLaunch {
                uniform_bound_x,
                uniform_bound_y,
                ..
            }) => Some((uniform_bound_x.clone(), uniform_bound_y.clone())),
            _ => None,
        })
        .expect("expected GpuLaunch terminator");
    assert!(
        launch.0.is_none() && launch.1.is_none(),
        "expected literal 2D bounds to have None uniform_bound_x and uniform_bound_y"
    );
}

#[test]
fn test_forall_gpu_2d_with_runtime_bounds_carries_uniform_bounds() {
    let body = mir_lower_code(
        "
use system.gpu
use system.collections.array

fn main()
    let w = 3
    let h = 4
    gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i, j in 0..w, 0..h
        dst[i * 4 + j] = i + j
",
    );
    let launch = body
        .basic_blocks
        .iter()
        .find_map(|bb| match bb.terminator.as_ref().map(|t| &t.kind) {
            Some(TerminatorKind::GpuLaunch {
                uniform_bound_x,
                uniform_bound_y,
                ..
            }) => Some((uniform_bound_x.is_some(), uniform_bound_y.is_some())),
            _ => None,
        })
        .expect("expected GpuLaunch terminator");
    assert!(
        launch.0 && launch.1,
        "expected runtime 2D bounds to carry both uniform_bound_x and uniform_bound_y"
    );
}

#[test]
fn test_forall_gpu_launch_terminator_carries_capture_args() {
    let body = mir_lower_code(
        "
use system.gpu
use system.collections.array

fn main()
    gpu let a = [1, 2, 3, 4]
    gpu let b = [5, 6, 7, 8]
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = a[i] + b[i]
",
    );
    let args = body
        .basic_blocks
        .iter()
        .find_map(|bb| match bb.terminator.as_ref().map(|t| &t.kind) {
            Some(TerminatorKind::GpuLaunch { launch_args, .. }) => {
                Some(launch_args.args().to_vec())
            }
            _ => None,
        })
        .expect("expected GpuLaunch terminator");
    assert!(
        !args.is_empty(),
        "GpuLaunch.args must be populated with capture operands, got empty"
    );
}

#[test]
fn test_forall_gpu_rejects_scalar_capture() {
    // Scalar captures are now supported: they are passed as WGSL uniforms
    // and appear in the kernel signature as UniformBuffer-class locals.
    // Verify that a scalar capture lowers correctly and appears in both
    // the parent function's GpuLaunch.scalar_args and the kernel's locals.
    let pipeline = Pipeline::new();
    let source = "
use system.gpu
use system.collections.array

fn main()
    let scale = 7
    gpu var dst = [0, 0, 0, 0]
    gpu forall i in 0..4
        dst[i] = scale
";
    let result = pipeline.frontend(source).expect("frontend");
    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|s| matches!(s.node, StatementKind::FunctionDeclaration(_)))
        .expect("a function declaration");
    let (body, lambdas) = lower_function(func_stmt, &result.type_checker, false, false)
        .expect("expected lowering to succeed for scalar capture");

    // Verify the parent function has a GpuLaunch terminator with scalar_args.
    let gpu_launch = body
        .basic_blocks
        .iter()
        .find_map(|bb| {
            bb.terminator.as_ref().and_then(|term| {
                if let TerminatorKind::GpuLaunch { scalar_args, .. } = &term.kind {
                    Some(scalar_args)
                } else {
                    None
                }
            })
        })
        .expect("expected a GpuLaunch terminator");

    // Verify the parent function's GpuLaunch has a scalar_args slot.
    assert!(
        !gpu_launch.is_empty(),
        "expected parent function to have scalar_args in GpuLaunch"
    );

    // Verify the kernel body has a UniformBuffer-class local for the scalar.
    let kernel = lambdas
        .first()
        .expect("expected at least one kernel lambda");
    let has_uniform_buffer_scalar = kernel.body.local_decls.iter().any(|decl| {
        decl.name.as_ref().map_or(false, |n| n.as_ref() == "scale")
            && matches!(decl.storage_class, StorageClass::UniformBuffer)
    });
    assert!(
        has_uniform_buffer_scalar,
        "expected kernel to have a UniformBuffer-class local named 'scale'"
    );
}

#[test]
fn test_forall_gpu_loops_in_different_functions_have_unique_kernel_names() {
    let names_a = synthesize_kernel_names(
        "
use system.gpu
use system.collections.array

fn a()
    gpu var a = [0, 0, 0, 0]
    gpu forall i in 0..4
        a[i] = i
",
    );
    let names_b = synthesize_kernel_names(
        "
use system.gpu
use system.collections.array

fn b()
    gpu var b = [0, 0, 0, 0]
    gpu forall i in 0..4
        b[i] = i
",
    );
    assert_eq!(names_a.len(), 1);
    assert_eq!(names_b.len(), 1);
    assert_ne!(
        names_a[0], names_b[0],
        "kernel names collide between functions: {} vs {}",
        names_a[0], names_b[0]
    );
}

#[test]
fn test_forall_gpu_3d_with_literal_bounds_has_none_uniform_bounds() {
    let body = mir_lower_code(
        "
use system.gpu
use system.collections.array

fn main()
    gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i, j, k in 0..2, 0..2, 0..2
        dst[i * 4 + j * 2 + k] = i + j + k
",
    );
    let launch = body
        .basic_blocks
        .iter()
        .find_map(|bb| match bb.terminator.as_ref().map(|t| &t.kind) {
            Some(TerminatorKind::GpuLaunch {
                uniform_bound_x,
                uniform_bound_y,
                uniform_bound_z,
                ..
            }) => Some((
                uniform_bound_x.clone(),
                uniform_bound_y.clone(),
                uniform_bound_z.clone(),
            )),
            _ => None,
        })
        .expect("expected GpuLaunch terminator");
    assert!(
        launch.0.is_none() && launch.1.is_none() && launch.2.is_none(),
        "expected literal 3D bounds to have None uniform_bound_x, uniform_bound_y, and uniform_bound_z"
    );
}

#[test]
fn test_forall_gpu_3d_with_runtime_bounds_carries_uniform_bounds() {
    let body = mir_lower_code(
        "
use system.gpu
use system.collections.array

fn main()
    let w = 2
    let h = 2
    let d = 2
    gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i, j, k in 0..w, 0..h, 0..d
        dst[i * 4 + j * 2 + k] = i + j + k
",
    );
    let launch = body
        .basic_blocks
        .iter()
        .find_map(|bb| match bb.terminator.as_ref().map(|t| &t.kind) {
            Some(TerminatorKind::GpuLaunch {
                uniform_bound_x,
                uniform_bound_y,
                uniform_bound_z,
                ..
            }) => Some((
                uniform_bound_x.is_some(),
                uniform_bound_y.is_some(),
                uniform_bound_z.is_some(),
            )),
            _ => None,
        })
        .expect("expected GpuLaunch terminator");
    assert!(
        launch.0 && launch.1 && launch.2,
        "expected runtime 3D bounds to carry uniform_bound_x, uniform_bound_y, and uniform_bound_z"
    );
}

#[test]
fn test_forall_gpu_kernel_workgroup_size_comes_from_backend_config() {
    // Test 1D kernel workgroup size.
    let pipeline = Pipeline::new();
    let source_1d = "
use system.gpu
use system.collections.array

fn main()
    gpu var a = [0, 0, 0, 0]
    gpu forall i in 0..4
        a[i] = i
";
    let result = pipeline.frontend(source_1d).expect("frontend");
    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|s| matches!(s.node, StatementKind::FunctionDeclaration(_)))
        .expect("function");
    let (_body, lambdas) =
        lower_function(func_stmt, &result.type_checker, false, false).expect("lower 1D");
    let kernel_1d = lambdas
        .iter()
        .find(|l| l.body.execution_model == ExecutionModel::GpuKernel)
        .expect("1D kernel");
    let metadata_1d = &kernel_1d.body.backend_metadata;
    assert!(
        matches!(metadata_1d, Some(miri::mir::backend::BackendMetadata::Gpu(md)) if md.workgroup_size == Some([256, 1, 1])),
        "expected 1D kernel to have workgroup_size [256, 1, 1], got {:?}",
        metadata_1d
    );

    // Test 2D kernel workgroup size.
    let source_2d = "
use system.gpu
use system.collections.array

fn main()
    gpu var a = [0, 0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i, j in 0..3, 0..3
        a[i * 3 + j] = i + j
";
    let result = pipeline.frontend(source_2d).expect("frontend");
    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|s| matches!(s.node, StatementKind::FunctionDeclaration(_)))
        .expect("function");
    let (_body, lambdas) =
        lower_function(func_stmt, &result.type_checker, false, false).expect("lower 2D");
    let kernel_2d = lambdas
        .iter()
        .find(|l| l.body.execution_model == ExecutionModel::GpuKernel)
        .expect("2D kernel");
    let metadata_2d = &kernel_2d.body.backend_metadata;
    assert!(
        matches!(metadata_2d, Some(miri::mir::backend::BackendMetadata::Gpu(md)) if md.workgroup_size == Some([16, 16, 1])),
        "expected 2D kernel to have workgroup_size [16, 16, 1], got {:?}",
        metadata_2d
    );

    // Test 3D kernel workgroup size.
    let source_3d = "
use system.gpu
use system.collections.array

fn main()
    gpu var a = [0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i, j, k in 0..2, 0..2, 0..2
        a[i * 4 + j * 2 + k] = i + j + k
";
    let result = pipeline.frontend(source_3d).expect("frontend");
    let func_stmt = result
        .ast
        .body
        .iter()
        .find(|s| matches!(s.node, StatementKind::FunctionDeclaration(_)))
        .expect("function");
    let (_body, lambdas) =
        lower_function(func_stmt, &result.type_checker, false, false).expect("lower 3D");
    let kernel_3d = lambdas
        .iter()
        .find(|l| l.body.execution_model == ExecutionModel::GpuKernel)
        .expect("3D kernel");
    let metadata_3d = &kernel_3d.body.backend_metadata;
    assert!(
        matches!(metadata_3d, Some(miri::mir::backend::BackendMetadata::Gpu(md)) if md.workgroup_size == Some([8, 8, 4])),
        "expected 3D kernel to have workgroup_size [8, 8, 4], got {:?}",
        metadata_3d
    );
}

#[test]
fn test_forall_gpu_2d_mixed_literal_runtime_carries_both_uniform_bounds() {
    let body = mir_lower_code(
        "
use system.gpu
use system.collections.array

fn main()
    let w = 5
    gpu var dst = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    gpu forall i, j in 0..3, 0..w
        dst[i * 5 + j] = i + j
",
    );
    let launch = body
        .basic_blocks
        .iter()
        .find_map(|bb| match bb.terminator.as_ref().map(|t| &t.kind) {
            Some(TerminatorKind::GpuLaunch {
                uniform_bound_x,
                uniform_bound_y,
                ..
            }) => Some((uniform_bound_x.is_some(), uniform_bound_y.is_some())),
            _ => None,
        })
        .expect("expected GpuLaunch terminator");
    assert!(
        launch.0 && launch.1,
        "expected mixed 2D bounds (literal x, runtime y) to carry both uniform_bound_x and uniform_bound_y"
    );
}
