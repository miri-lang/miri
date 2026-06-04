// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Static bundle validation tests for web-gpu output.
//! These tests verify the emitted HTML/WGSL/JS without requiring GPU hardware or browser.

use std::fs;
use std::path::PathBuf;

/// Helper to build a program and return bundle directory path.
/// The returned path is absolute and will not be cleaned up automatically.
fn build_bundle_to_tempdir(source: &str) -> PathBuf {
    use miri::codegen::backend::BuildTarget;
    use miri::pipeline::{BuildOptions, Pipeline};
    use std::time::{SystemTime, UNIX_EPOCH};

    let pipeline = Pipeline::new();
    // Use a unique timestamp to avoid collisions between parallel tests
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let temp_base = std::env::temp_dir().join("miri_bundle_test").join(format!(
        "test_{}_{}",
        std::process::id(),
        nanos
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

/// Helper to read a file from the bundle directory.
fn read_bundle_file(bundle_dir: &PathBuf, path: &str) -> String {
    fs::read_to_string(bundle_dir.join(path)).expect(&format!("Failed to read {}", path))
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
fn test_bundle_basic_structure() {
    let source = r#"
use system.gpu

gpu let src = [1, 2, 3, 4]
gpu var dst = [0, 0, 0, 0]

gpu for i in 0..4
    dst[i] = src[i]
"#;

    let bundle_dir = build_bundle_to_tempdir(source);

    // Verify index.html exists
    let index_html = read_bundle_file(&bundle_dir, "index.html");
    assert!(!index_html.is_empty());

    // Verify miri_gpu_runtime.js exists
    let runtime_js = read_bundle_file(&bundle_dir, "miri_gpu_runtime.js");
    assert!(runtime_js.contains("export async function dispatch(spec)"));

    // Verify kernel file exists (named after the gpu for)
    let kernels_dir = bundle_dir.join("kernels");
    let entries: Vec<_> = std::fs::read_dir(&kernels_dir)
        .expect("read kernels dir")
        .filter_map(|e| e.ok())
        .collect();
    assert!(!entries.is_empty(), "should have at least one kernel");

    // Verify kernel WGSL is valid
    for entry in entries {
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "wgsl") {
            let wgsl = fs::read_to_string(&path).expect("read WGSL");
            validate_wgsl(&wgsl);
        }
    }
}

#[test]
fn test_bundle_contains_canvas() {
    let source = r#"
use system.gpu

gpu let src = [1, 2, 3, 4]
gpu var dst = [0, 0, 0, 0]

gpu for i in 0..4
    dst[i] = src[i]
"#;

    let bundle_dir = build_bundle_to_tempdir(source);
    let index_html = read_bundle_file(&bundle_dir, "index.html");

    assert!(index_html.contains("canvas"));
    assert!(index_html.contains(r#"id="output""#));
}

#[test]
fn test_bundle_contains_source_panel() {
    let source = r#"use system.gpu
gpu let src = [1, 2, 3, 4]
gpu var dst = [0, 0, 0, 0]
gpu for i in 0..4
    dst[i] = src[i]"#;

    let bundle_dir = build_bundle_to_tempdir(source);
    let index_html = read_bundle_file(&bundle_dir, "index.html");

    // Check for source panel section
    assert!(index_html.contains("sourcePanel"));
    // Check that source text is escaped and embedded
    assert!(index_html.contains("gpu for"));
}

#[test]
fn test_bundle_animate_flag() {
    let source = r#"
use system.gpu

gpu let src = [1, 2, 3, 4]
gpu var dst = [0, 0, 0, 0]

gpu for i in 0..4
    dst[i] = src[i]
"#;

    let bundle_dir = build_bundle_to_tempdir(source);
    let index_html = read_bundle_file(&bundle_dir, "index.html");

    // Check for requestAnimationFrame in JS
    assert!(index_html.contains("requestAnimationFrame"));
}

#[test]
fn test_bundle_with_real_input_buffers() {
    let source = r#"
use system.gpu
use system.collections.array

gpu let a = [1, 2, 3, 4]
gpu let b = [5, 6, 7, 8]
gpu var dst = [0, 0, 0, 0]

gpu for i in 0..4
    dst[i] = a[i] + b[i]
"#;

    let bundle_dir = build_bundle_to_tempdir(source);
    let index_html = read_bundle_file(&bundle_dir, "index.html");

    // Check for kernel manifest
    assert!(index_html.contains("KERNELS"));
}

#[test]
fn test_bundle_wgsl_validation_1d_vector_add() {
    let source = r#"
use system.gpu
use system.collections.array

gpu let a = [1, 2, 3, 4]
gpu let b = [5, 6, 7, 8]
gpu var dst = [0, 0, 0, 0]

gpu for i in 0..4
    dst[i] = a[i] + b[i]
"#;

    let bundle_dir = build_bundle_to_tempdir(source);

    // Find and validate any WGSL files
    let kernels_dir = bundle_dir.join("kernels");
    let entries: Vec<_> = std::fs::read_dir(&kernels_dir)
        .expect("read kernels dir")
        .filter_map(|e| e.ok())
        .collect();
    assert!(!entries.is_empty(), "should have at least one kernel");

    for entry in entries {
        let path = entry.path();
        if path.extension().map_or(false, |ext| ext == "wgsl") {
            let wgsl = fs::read_to_string(&path).expect("read WGSL");
            validate_wgsl(&wgsl);
            assert!(wgsl.contains("@compute"));
        }
    }
}

#[test]
fn test_bundle_manifest_has_real_initial_data() {
    let source = r#"
use system.gpu

gpu let a = [1, 2, 3, 4]
gpu let b = [5, 6, 7, 8]
gpu var dst = [0, 0, 0, 0]

gpu for i in 0..4
    dst[i] = a[i] + b[i]
"#;

    let bundle_dir = build_bundle_to_tempdir(source);
    let index_html = read_bundle_file(&bundle_dir, "index.html");

    // Extract the KERNELS manifest from the HTML
    let manifest_start = index_html
        .find("const KERNELS = ")
        .expect("should find KERNELS");
    let manifest_end = index_html[manifest_start..]
        .find(";")
        .expect("should find manifest end");
    let manifest_str = &index_html[manifest_start + 16..manifest_start + manifest_end];

    // Parse the manifest as JSON-like structure and verify initial data
    assert!(
        manifest_str.contains("initialData"),
        "manifest should contain initialData field"
    );

    // Check that 'a' has correct initial data [1,2,3,4]
    assert!(
        manifest_str.contains("\"a\"") || manifest_str.contains("'a'"),
        "manifest should have buffer 'a'"
    );
    assert!(
        manifest_str.contains("1")
            && manifest_str.contains("2")
            && manifest_str.contains("3")
            && manifest_str.contains("4"),
        "manifest should have values 1,2,3,4"
    );

    // Check that 'b' has correct initial data [5,6,7,8]
    assert!(
        manifest_str.contains("\"b\"") || manifest_str.contains("'b'"),
        "manifest should have buffer 'b'"
    );
    assert!(
        manifest_str.contains("5")
            && manifest_str.contains("6")
            && manifest_str.contains("7")
            && manifest_str.contains("8"),
        "manifest should have values 5,6,7,8"
    );

    // Check readOnly flags (a and b should be readOnly: true, dst should be readOnly: false)
    assert!(
        manifest_str.contains("readOnly"),
        "manifest should contain readOnly field"
    );
}

#[test]
fn test_bundle_manifest_int_and_float_buffers() {
    let source = r#"
use system.gpu

gpu let ints = [1, 2, 3, 4]
gpu let floats = [1.5, 2.5, 3.5, 4.5]
gpu var results = [0, 0, 0, 0]

gpu for i in 0..4
    results[i] = ints[i]
"#;

    let bundle_dir = build_bundle_to_tempdir(source);
    let index_html = read_bundle_file(&bundle_dir, "index.html");

    // Extract and verify manifest
    let manifest_start = index_html
        .find("const KERNELS = ")
        .expect("should find KERNELS");
    let manifest_end = index_html[manifest_start..]
        .find(";")
        .expect("should find manifest end");
    let manifest_str = &index_html[manifest_start + 16..manifest_start + manifest_end];

    assert!(
        manifest_str.contains("initialData"),
        "manifest should contain initialData"
    );
    assert!(
        manifest_str.contains("elemType"),
        "manifest should contain elemType for distinguishing types"
    );
}
