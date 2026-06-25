// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Manifest and WGSL validation tests for web-GPU bundles.
//!
//! The bundle format consists of:
//! - <name>.json manifest with kernel specs, buffers, and animation metadata
//! - miri-gpu.js runtime driver (ES module)
//! - index.html thin harness for local development
//! - Per-kernel WGSL embedded in the manifest
//!
//! Tests verify:
//! - Per-kernel WGSL passes naga validation
//! - Manifest correctly reflects buffer types, sizes, and initial data
//! - Sized-constructor arrays (Array<T, N>()) have correct elem types
//! - Float and integer buffers are properly distinguished

use std::fs;
use std::path::PathBuf;

/// Helper to build a program and return bundle directory path.
/// The returned path is absolute and will not be cleaned up automatically.
fn build_bundle_to_tempdir(source: &str) -> PathBuf {
    use miri::codegen::backend::BuildTarget;
    use miri::pipeline::{BuildOptions, Pipeline};
    use std::sync::atomic::{AtomicU64, Ordering};

    let pipeline = Pipeline::new();
    // Use a process-wide monotonic counter to avoid collisions between parallel tests
    static BUNDLE_DIR_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = BUNDLE_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let temp_base = std::env::temp_dir().join("miri_bundle_test").join(format!(
        "test_{}_{}",
        std::process::id(),
        seq
    ));
    fs::create_dir_all(&temp_base).expect("create test dir");

    let opts = BuildOptions {
        target: BuildTarget::WebGpu,
        out_path: Some(temp_base.clone()),
        release: false,
        opt_level: 0,
        cpu_backend: Default::default(),
    };

    let _index_html_path = pipeline.build(source, &opts).expect("build should succeed");

    // emit_bundle writes to out_path directly, so the bundle dir is out_path itself
    temp_base
}

/// Helper to read a JSON manifest from the bundle directory.
fn read_manifest(bundle_dir: &PathBuf) -> serde_json::Value {
    // The manifest name is derived from the bundle directory name
    let dir_name = bundle_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("bundle");
    let manifest_path = bundle_dir.join(format!("{}.json", dir_name));
    let manifest_text = fs::read_to_string(&manifest_path).expect("read manifest");
    serde_json::from_str(&manifest_text).expect("parse manifest JSON")
}

/// Helper to validate WGSL with naga.
fn validate_wgsl(wgsl: &str) {
    let module = naga::front::wgsl::parse_str(wgsl).unwrap_or_else(|err| {
        panic!(
            "naga parse failed: {}\nWGSL:\n{}",
            err.emit_to_string(wgsl),
            wgsl
        )
    });
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator
        .validate(&module)
        .unwrap_or_else(|err| panic!("naga validate failed: {:?}\nWGSL:\n{}", err, wgsl));
}

#[test]
fn manifest_emits_initialdata_on_all_buffers() {
    let source = r#"
use system.gpu

gpu let a = [1, 2, 3, 4]
gpu let b = [5, 6, 7, 8]
gpu var dst = [0, 0, 0, 0]

gpu forall i in 0..4
    dst[i] = a[i] + b[i]
"#;

    let bundle_dir = build_bundle_to_tempdir(source);
    let manifest = read_manifest(&bundle_dir);

    // Every buffer must have an initialData field (even if null)
    let buffers = manifest["buffers"].as_array().expect("buffers is array");
    assert!(!buffers.is_empty(), "manifest should have buffers");

    for buf in buffers {
        assert!(
            buf.get("initialData").is_some(),
            "every buffer must have initialData field. buffer: {}",
            serde_json::to_string(&buf).unwrap()
        );
    }
}

#[test]
fn manifest_has_literal_initial_data() {
    let source = r#"
use system.gpu

gpu let a = [1, 2, 3, 4]
gpu let b = [5, 6, 7, 8]
gpu var dst = [0, 0, 0, 0]

gpu forall i in 0..4
    dst[i] = a[i] + b[i]
"#;

    let bundle_dir = build_bundle_to_tempdir(source);
    let manifest = read_manifest(&bundle_dir);

    let buffers: Vec<_> = manifest["buffers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (b["name"].as_str().unwrap(), b))
        .collect();

    // Find buffer 'a'
    if let Some((_, buf_a)) = buffers.iter().find(|(name, _)| *name == "a") {
        let init_data = &buf_a["initialData"];
        assert!(
            !init_data.is_null(),
            "buffer 'a' should have literal initialData, not null"
        );
        let data = init_data.as_array().expect("initialData is array");
        assert_eq!(data.len(), 4, "buffer 'a' should have 4 elements");
        // Check that values are [1, 2, 3, 4] (as JSON numbers)
        assert_eq!(data[0], 1, "first element of 'a'");
        assert_eq!(data[1], 2, "second element of 'a'");
        assert_eq!(data[2], 3, "third element of 'a'");
        assert_eq!(data[3], 4, "fourth element of 'a'");
    }

    // Find buffer 'b'
    if let Some((_, buf_b)) = buffers.iter().find(|(name, _)| *name == "b") {
        let init_data = &buf_b["initialData"];
        assert!(
            !init_data.is_null(),
            "buffer 'b' should have literal initialData, not null"
        );
        let data = init_data.as_array().expect("initialData is array");
        assert_eq!(data.len(), 4, "buffer 'b' should have 4 elements");
        // Check that values are [5, 6, 7, 8]
        assert_eq!(data[0], 5, "first element of 'b'");
        assert_eq!(data[1], 6, "second element of 'b'");
        assert_eq!(data[2], 7, "third element of 'b'");
        assert_eq!(data[3], 8, "fourth element of 'b'");
    }
}

#[test]
fn manifest_has_null_initial_data_for_zero_filled() {
    let source = r#"
use system.gpu
use system.collections.array

gpu var px = Array<f32, 8>()

gpu forall i in 0..8
    px[i] = i as f32
"#;

    let bundle_dir = build_bundle_to_tempdir(source);
    let manifest = read_manifest(&bundle_dir);

    let buffers = manifest["buffers"].as_array().expect("buffers is array");
    assert!(!buffers.is_empty(), "manifest should have buffers");

    // Sized constructors should have null initialData
    for buf in buffers {
        let init_data = &buf["initialData"];
        assert!(
            init_data.is_null(),
            "zero-filled buffer should have null initialData, got: {}",
            init_data
        );
    }
}

#[test]
fn manifest_elem_types_match_buffer_types() {
    let source = r#"
use system.gpu
use system.collections.array

gpu let ints = [1, 2, 3, 4]
gpu var floats = Array<f32, 4>()

gpu forall i in 0..4
    floats[i] = ints[i] as f32
"#;

    let bundle_dir = build_bundle_to_tempdir(source);
    let manifest = read_manifest(&bundle_dir);

    let buffers: Vec<_> = manifest["buffers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (b["name"].as_str().unwrap(), b))
        .collect();

    // Check integer buffer
    if let Some((_, buf_ints)) = buffers.iter().find(|(name, _)| *name == "ints") {
        let elem_type = buf_ints["elemType"].as_str().unwrap();
        assert_eq!(elem_type, "i32", "integer buffer should have elemType i32");
    }

    // Check float buffer
    if let Some((_, buf_floats)) = buffers.iter().find(|(name, _)| *name == "floats") {
        let elem_type = buf_floats["elemType"].as_str().unwrap();
        assert_eq!(elem_type, "f32", "f32 buffer should have elemType f32");
    }
}

#[test]
fn kernel_wgsl_in_manifest_passes_validation() {
    let source = r#"
use system.gpu

gpu let a = [1, 2, 3, 4]
gpu let b = [5, 6, 7, 8]
gpu var dst = [0, 0, 0, 0]

gpu forall i in 0..4
    dst[i] = a[i] + b[i]
"#;

    let bundle_dir = build_bundle_to_tempdir(source);
    let manifest = read_manifest(&bundle_dir);

    // Validate seed kernel WGSL
    if let Some(seed_array) = manifest["seed"].as_array() {
        for kernel in seed_array {
            let wgsl = kernel["wgsl"].as_str().expect("wgsl is string");
            validate_wgsl(wgsl);
            assert!(
                wgsl.contains("@compute"),
                "kernel WGSL should have @compute attribute"
            );
        }
    }

    // Validate frame kernel WGSL if present
    if let Some(frame) = manifest["frame"].as_object() {
        let wgsl = frame["wgsl"].as_str().expect("wgsl is string");
        validate_wgsl(wgsl);
        assert!(
            wgsl.contains("@compute"),
            "frame kernel WGSL should have @compute attribute"
        );
    }
}

#[test]
fn kernel_workgroups_is_dispatch_grid() {
    let source = r#"
use system.gpu
use system.collections.array

gpu let src = Array<int, 4096>()
gpu var dst = Array<int, 4096>()

gpu forall i in 0..4096
    dst[i] = src[i]
"#;

    let bundle_dir = build_bundle_to_tempdir(source);
    let manifest = read_manifest(&bundle_dir);

    // For 4096 elements with 256-thread workgroups, grid should be ceil(4096/256) = 16
    if let Some(seed_array) = manifest["seed"].as_array() {
        for kernel in seed_array {
            let workgroups = kernel["workgroups"]
                .as_array()
                .expect("workgroups is array");
            assert_eq!(workgroups.len(), 3, "workgroups should be [x, y, z]");
            let grid_x = workgroups[0].as_u64().expect("grid x is number");
            assert_eq!(
                grid_x, 16,
                "dispatch grid for 4096 elements / 256 threads should be 16"
            );
            assert_eq!(workgroups[1], 1, "grid y should be 1");
            assert_eq!(workgroups[2], 1, "grid z should be 1");
        }
    }
}

/// F31 verification: Two gpu forall loops produce distinct kernel entries.
/// Ground truth: AST statement IDs are globally unique (not per-function),
/// so kernel names are already distinct. This test verifies the fix.
#[test]
fn two_gpu_for_loops_produce_distinct_kernels() {
    let source = r#"
use system.gpu

gpu var dst_a = [0, 0, 0, 0]
gpu var dst_b = [0, 0, 0, 0]

gpu forall i in 0..4
    dst_a[i] = i + 100

gpu forall i in 0..4
    dst_b[i] = i + 200
"#;

    let bundle_dir = build_bundle_to_tempdir(source);
    let manifest = read_manifest(&bundle_dir);

    // Extract all kernels from the manifest
    let seed_array = manifest["seed"].as_array().expect("seed is array");

    assert_eq!(
        seed_array.len(),
        2,
        "Expected 2 kernels for two gpu forall loops; got {}",
        seed_array.len()
    );

    // Build a map of kernel entry points to their WGSL
    let mut kernels: Vec<(String, String)> = Vec::new();
    for kernel in seed_array {
        let entry_point = kernel["entryPoint"]
            .as_str()
            .expect("entryPoint is string")
            .to_string();
        let wgsl = kernel["wgsl"].as_str().expect("wgsl is string").to_string();
        kernels.push((entry_point, wgsl));
    }

    // Verify kernels are distinct
    assert_ne!(
        kernels[0].0, kernels[1].0,
        "Kernel entry points should be distinct; both are '{}' (collision)",
        kernels[0].0
    );

    // Validate each kernel's WGSL independently
    validate_wgsl(&kernels[0].1);
    validate_wgsl(&kernels[1].1);

    // Verify distinguishing constants appear in the expected kernels.
    // First kernel computes `i + 100`, second computes `i + 200`.
    // These literals should appear in the WGSL as constants or immediates.
    assert!(
        kernels[0].1.contains("100"),
        "First kernel should contain constant 100; WGSL: {}",
        kernels[0].1
    );
    assert!(
        kernels[1].1.contains("200"),
        "Second kernel should contain constant 200; WGSL: {}",
        kernels[1].1
    );
}

/// A kernel that calls a device-side helper `fn` must emit that helper's
/// definition into its own WGSL module, or the browser validator rejects the
/// call as an unknown identifier. The native kernel registry already bundles
/// every reachable `GpuDevice` helper into each kernel module; the web bundle
/// must do the same.
#[test]
fn kernel_wgsl_in_manifest_includes_device_helpers() {
    let source = r#"
use system.collections.array
use system.math

fn doubled(x f32) f32
    return x * 2.0

gpu var dst = Array<f32, 4>()

gpu forall i in 0..4
    dst[i] = doubled(i as f32)
"#;

    let bundle_dir = build_bundle_to_tempdir(source);
    let manifest = read_manifest(&bundle_dir);

    let seed_array = manifest["seed"].as_array().expect("seed is array");
    assert!(!seed_array.is_empty(), "expected at least one seed kernel");

    for kernel in seed_array {
        let wgsl = kernel["wgsl"].as_str().expect("wgsl is string");
        // The call site must resolve: the helper definition is present and the
        // whole module passes the WGSL validator.
        assert!(
            wgsl.contains("fn doubled("),
            "kernel WGSL must define the called helper `doubled`; WGSL:\n{}",
            wgsl
        );
        validate_wgsl(wgsl);
    }
}
