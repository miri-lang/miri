// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// `miri build --target web-gpu` produces a bundle:
//   - <name>.json     — manifest with buffers, kernels, and animation metadata
//   - miri-gpu.js     — reusable embeddable runtime driver (copied from assets/web/)
//   - index.html      — thin harness for local development
// The manifest is the primary artifact, consumed by miri-gpu.js.

use crate::utils::miri_cmd;
use std::fs;
use std::io::Write;
use tempfile::NamedTempFile;

const GPU_FOR_SOURCE: &str = r#"use system.io
use system.gpu
use system.collections.array

gpu let a = [1, 2, 3, 4]
gpu let b = [10, 20, 30, 40]
gpu var dst = [0, 0, 0, 0]
gpu forall i in 0..4
    dst[i] = a[i] + b[i]
println("done")
"#;

const GPU_FRAME_SOURCE: &str = r#"use system.io
use system.gpu
use system.collections.array

gpu var grid_a = Array<int, 16>()
gpu var grid_b = Array<int, 16>()

gpu forall idx in 0..16
    grid_a[idx] = idx as int

gpu frame idx in 0..16
    grid_b[idx] = grid_a[idx] + 1
"#;

fn write_source(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", content).unwrap();
    file
}

#[test]
fn target_web_gpu_emits_manifest_and_runtime() {
    let source = write_source(GPU_FOR_SOURCE);
    let out_dir = tempfile::tempdir().unwrap();
    let bundle_dir = out_dir.path().join("bundle");

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(source.path())
        .arg("--target")
        .arg("web-gpu")
        .arg("--out")
        .arg(&bundle_dir)
        .assert()
        .success();

    // Check manifest file exists and parses as valid JSON
    // The manifest name is derived from the output directory name
    let manifest_path = bundle_dir.join("bundle.json");
    assert!(
        manifest_path.is_file(),
        "expected manifest at {:?} (directory contents: {:?})",
        manifest_path,
        fs::read_dir(&bundle_dir)
            .ok()
            .map(|mut entries| entries.next())
    );

    let manifest_text = fs::read_to_string(&manifest_path).expect("read manifest");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("parse manifest JSON");

    // Validate manifest schema
    assert!(
        manifest["name"].is_string(),
        "manifest must have 'name' string field"
    );
    assert!(
        manifest["canvas"]["width"].is_number(),
        "manifest must have canvas.width number"
    );
    assert!(
        manifest["canvas"]["height"].is_number(),
        "manifest must have canvas.height number"
    );
    assert!(
        manifest["buffers"].is_array(),
        "manifest must have 'buffers' array"
    );
    assert!(
        manifest["seed"].is_array(),
        "manifest must have 'seed' array"
    );
    assert!(
        manifest["paint"].is_string(),
        "manifest must have 'paint' string (buffer name)"
    );

    // Validate kernel specs
    let seed = &manifest["seed"];
    assert!(
        !seed.as_array().unwrap().is_empty(),
        "seed kernels must be non-empty"
    );
    for kernel in seed.as_array().unwrap() {
        assert!(kernel["entryPoint"].is_string(), "kernel needs entryPoint");
        assert!(kernel["wgsl"].is_string(), "kernel needs wgsl source");
        assert!(kernel["workgroups"].is_array(), "kernel needs workgroups");
        assert!(kernel["bindings"].is_array(), "kernel needs bindings");
        assert!(
            !kernel["wgsl"].as_str().unwrap().contains("i64"),
            "WGSL must not contain i64 (unsupported on some targets)"
        );
    }

    // Check miri-gpu.js runtime is copied
    let runtime_path = bundle_dir.join("miri-gpu.js");
    assert!(
        runtime_path.is_file(),
        "expected miri-gpu.js runtime copied to {:?}",
        runtime_path
    );
    let runtime_text = fs::read_to_string(&runtime_path).expect("read runtime");
    assert!(
        runtime_text.contains("export async function mount"),
        "miri-gpu.js must export mount function"
    );

    // Check index.html harness
    let index_path = bundle_dir.join("index.html");
    assert!(
        index_path.is_file(),
        "expected index.html at {:?}",
        index_path
    );
    let index_text = fs::read_to_string(&index_path).expect("read index.html");
    assert!(
        index_text.contains("<canvas"),
        "index.html must have <canvas>"
    );
    assert!(
        index_text.contains("import { mount } from"),
        "index.html must import mount from miri-gpu.js"
    );
    assert!(
        index_text.contains(".json"),
        "index.html must reference the manifest JSON file"
    );
}

#[test]
fn target_web_gpu_frame_kernel_in_manifest() {
    let source = write_source(GPU_FRAME_SOURCE);
    let out_dir = tempfile::tempdir().unwrap();
    let bundle_dir = out_dir.path().join("bundle");

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(source.path())
        .arg("--target")
        .arg("web-gpu")
        .arg("--out")
        .arg(&bundle_dir)
        .assert()
        .success();

    let manifest_path = bundle_dir.join("bundle.json");
    let manifest_text = fs::read_to_string(&manifest_path).expect("read manifest");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("parse manifest JSON");

    // Validate framePasses field is present
    assert!(
        manifest["framePasses"].is_array(),
        "manifest 'framePasses' must be an array"
    );

    // Validate each frame pass has read/write and bindings
    let frame_passes = manifest["framePasses"].as_array().unwrap();
    for frame in frame_passes {
        assert!(frame.is_object(), "each frame pass must be an object");
        assert!(
            frame["read"].is_string() || frame["read"].is_null(),
            "frame pass must have 'read' buffer name"
        );
        assert!(
            frame["write"].is_string() || frame["write"].is_null(),
            "frame pass must have 'write' buffer name"
        );

        // Validate bindings match the read/write buffers
        let bindings = &frame["bindings"];
        assert!(bindings.is_array(), "frame pass must have bindings array");

        // Check that binding access matches frame.read/write classification
        let read_buf = frame["read"].as_str();
        let write_buf = frame["write"].as_str();

        for binding in bindings.as_array().unwrap() {
            let name = binding["name"].as_str().unwrap();
            let access = binding["access"].as_str().unwrap();

            if Some(name) == read_buf {
                assert_eq!(
                    access, "read",
                    "read buffer binding must have access='read'"
                );
            } else if Some(name) == write_buf {
                assert_eq!(
                    access, "read_write",
                    "write buffer binding must have access='read_write'"
                );
            }
        }
    }
}

#[test]
fn target_web_gpu_rejects_program_without_gpu_kernels() {
    let source = write_source(
        r#"use system.io
println("just host code")
"#,
    );
    let out_dir = tempfile::tempdir().unwrap();
    let bundle_dir = out_dir.path().join("bundle");

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(source.path())
        .arg("--target")
        .arg("web-gpu")
        .arg("--out")
        .arg(&bundle_dir)
        .assert()
        .failure()
        .stderr(predicates::str::contains("no GPU kernels"));
}

#[test]
fn manifest_paint_mode_rgba_for_f32_4x_buffer() {
    const RGBA_SOURCE: &str = r#"use system.io
use system.gpu
use system.collections.array

gpu var canvas = Array<f32, 64>()

gpu forall i in 0..16
    canvas[i * 4 + 0] = 1.0
    canvas[i * 4 + 1] = 0.5
    canvas[i * 4 + 2] = 0.25
    canvas[i * 4 + 3] = 1.0
"#;

    let source = write_source(RGBA_SOURCE);
    let out_dir = tempfile::tempdir().unwrap();
    let bundle_dir = out_dir.path().join("bundle");

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(source.path())
        .arg("--target")
        .arg("web-gpu")
        .arg("--out")
        .arg(&bundle_dir)
        .assert()
        .success();

    let manifest_path = bundle_dir.join("bundle.json");
    let manifest_text = fs::read_to_string(&manifest_path).expect("read manifest");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("parse manifest JSON");

    assert!(
        manifest["paintMode"].is_string(),
        "manifest should have 'paintMode' for RGBA buffer"
    );
    assert_eq!(
        manifest["paintMode"].as_str().unwrap(),
        "rgba",
        "paintMode should be 'rgba' for f32 4x buffer"
    );
}

#[test]
fn manifest_paint_mode_default_colormap_for_int_buffer() {
    const COLORMAP_SOURCE: &str = r#"use system.io
use system.gpu
use system.collections.array

gpu var canvas = Array<int, 16>()

gpu forall i in 0..16
    canvas[i] = i
"#;

    let source = write_source(COLORMAP_SOURCE);
    let out_dir = tempfile::tempdir().unwrap();
    let bundle_dir = out_dir.path().join("bundle");

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(source.path())
        .arg("--target")
        .arg("web-gpu")
        .arg("--out")
        .arg(&bundle_dir)
        .assert()
        .success();

    let manifest_path = bundle_dir.join("bundle.json");
    let manifest_text = fs::read_to_string(&manifest_path).expect("read manifest");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("parse manifest JSON");

    assert!(
        manifest["paintMode"].is_null(),
        "manifest should have null/omitted 'paintMode' for non-RGBA buffer (defaults to colormap)"
    );
}

#[test]
fn frame_kernel_inputs_manifest_field() {
    // D4: verify frame kernels have inputs field in manifest with correct offsets
    const FRAME_WITH_FIELDS_SOURCE: &str = r#"use system.io
use system.gpu
use system.collections.array

gpu var grid_a = Array<int, 16>()
gpu var grid_b = Array<int, 16>()

gpu forall idx in 0..16
    grid_a[idx] = idx as int

gpu frame idx in 0..16
    if frame.mouse_down:
        let t = frame.time
        grid_b[idx] = grid_a[idx] + 1
"#;

    let source = write_source(FRAME_WITH_FIELDS_SOURCE);
    let out_dir = tempfile::tempdir().unwrap();
    let bundle_dir = out_dir.path().join("bundle");

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(source.path())
        .arg("--target")
        .arg("web-gpu")
        .arg("--out")
        .arg(&bundle_dir)
        .assert()
        .success();

    let manifest_path = bundle_dir.join("bundle.json");
    let manifest_text = fs::read_to_string(&manifest_path).expect("read manifest");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("parse manifest JSON");

    // Verify framePasses array exists and has at least one pass
    assert!(
        manifest["framePasses"].is_array(),
        "manifest should have framePasses array"
    );

    let frame_passes = manifest["framePasses"].as_array().unwrap();
    assert!(
        frame_passes.len() >= 1,
        "framePasses must have at least one pass"
    );

    // Check first pass has frame inputs
    let frame = &frame_passes[0];

    // Verify inputs field exists
    assert!(
        frame["inputs"].is_array(),
        "frame pass should have 'inputs' array field"
    );

    let inputs = frame["inputs"].as_array().unwrap();
    assert_eq!(
        inputs.len(),
        11,
        "frame pass inputs must have exactly 11 fields"
    );

    // Verify canonical order and offsets
    let expected_fields = [
        ("time", "f32", 0),
        ("dt", "f32", 4),
        ("index", "i32", 8),
        ("mouse_x", "f32", 12),
        ("mouse_y", "f32", 16),
        ("mouse_down", "u32", 20),
        ("drag_dx", "f32", 24),
        ("drag_dy", "f32", 28),
        ("wheel", "f32", 32),
        ("clicked", "u32", 36),
        ("double_clicked", "u32", 40),
    ];

    for (i, (name, ty, offset)) in expected_fields.iter().enumerate() {
        let input = &inputs[i];
        assert_eq!(
            input["name"].as_str().unwrap(),
            *name,
            "input[{}].name should be '{}'",
            i,
            name
        );
        assert_eq!(
            input["ty"].as_str().unwrap(),
            *ty,
            "input[{}].ty should be '{}'",
            i,
            ty
        );
        assert_eq!(
            input["offset"].as_u64().unwrap(),
            *offset as u64,
            "input[{}].offset should be {}",
            i,
            offset
        );
    }
}

#[test]
fn non_frame_kernel_no_inputs_field() {
    // Verify non-frame kernels omit the inputs field entirely
    let source = write_source(GPU_FOR_SOURCE);
    let out_dir = tempfile::tempdir().unwrap();
    let bundle_dir = out_dir.path().join("bundle");

    let mut cmd = miri_cmd();
    cmd.arg("build")
        .arg(source.path())
        .arg("--target")
        .arg("web-gpu")
        .arg("--out")
        .arg(&bundle_dir)
        .assert()
        .success();

    let manifest_path = bundle_dir.join("bundle.json");
    let manifest_text = fs::read_to_string(&manifest_path).expect("read manifest");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_text).expect("parse manifest JSON");

    // Check seed kernels do not have inputs field (prove skip_serializing_if works)
    let seed = manifest["seed"].as_array().unwrap();
    assert!(!seed.is_empty(), "should have at least one seed kernel");
    for kernel in seed {
        assert!(
            kernel.get("inputs").is_none(),
            "non-frame kernel should not have inputs field in JSON"
        );
    }
}
