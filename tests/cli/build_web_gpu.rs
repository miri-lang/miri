// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// `miri build --target web-gpu` produces a browser-runnable bundle:
//   - index.html       — harness with <canvas>, loads the JS runtime + WGSL.
//   - miri_gpu_runtime.js — copied from assets/web/.
//   - kernels/*.wgsl   — one shader per `gpu fn` kernel emitted from the source.
// The native object/binary path is skipped — the bundle is the artifact.

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
gpu for i in 0..4
    dst[i] = a[i] + b[i]
println("done")
"#;

fn write_source(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    write!(file, "{}", content).unwrap();
    file
}

#[test]
fn target_web_gpu_emits_html_bundle_with_canvas_and_kernel() {
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

    let index_path = bundle_dir.join("index.html");
    let index = fs::read_to_string(&index_path)
        .unwrap_or_else(|_| panic!("expected bundle index.html at {:?}", index_path));
    assert!(
        index.contains("<canvas"),
        "index.html must declare a <canvas> for compute output rendering, got:\n{}",
        index
    );
    assert!(
        index.contains("miri_gpu_runtime.js"),
        "index.html must reference the JS runtime, got:\n{}",
        index
    );

    let runtime_path = bundle_dir.join("miri_gpu_runtime.js");
    assert!(
        runtime_path.is_file(),
        "expected runtime shim copied to {:?}",
        runtime_path
    );

    let kernels_dir = bundle_dir.join("kernels");
    let entries: Vec<_> = fs::read_dir(&kernels_dir)
        .unwrap_or_else(|_| panic!("expected kernels/ at {:?}", kernels_dir))
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "wgsl"))
        .collect();
    assert!(
        !entries.is_empty(),
        "expected at least one .wgsl kernel file emitted in {:?}",
        kernels_dir
    );

    let wgsl = fs::read_to_string(entries[0].path()).unwrap();
    assert!(
        wgsl.contains("@compute"),
        "kernel file must contain @compute entry point, got:\n{}",
        wgsl
    );
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
