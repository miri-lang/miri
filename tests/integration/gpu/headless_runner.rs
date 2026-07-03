// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Headless-runner smoke tests for `--target web-gpu` bundles.
//!
//! A web-gpu bundle ships `miri-gpu-headless.js` (a CLI driver) and a
//! `package.json` ES-module marker alongside `miri-gpu.js` + the manifest, so a
//! WebGPU-capable JS runtime (Deno, or a WebGPU-enabled Node) can boot the
//! bundle without a browser and verify it actually loads, uploads, dispatches,
//! and reads back — the CI smoke path a real browser cannot provide on headless
//! CI.
//!
//! Two layers:
//! - Always-run plumbing: the runner + marker are emitted and wired to
//!   `runHeadless`. Needs no JS runtime and no GPU.
//! - Runtime-driven boot smoke: run the emitted runner under a JS runtime.
//!   Resolution mirrors the tint gate (`MIRI_NODE` / `MIRI_DENO` env, then
//!   PATH); the test skips gracefully when no runtime is installed. Under a
//!   GPU-less runtime (plain Node) the bundle boots up to the device request
//!   and fails with `WebGPU unavailable`; under a WebGPU runtime it dispatches
//!   and returns the readback values.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Build a web-gpu bundle for `source` and return its directory.
fn build_bundle(source: &str) -> PathBuf {
    use miri::codegen::backend::BuildTarget;
    use miri::pipeline::{BuildOptions, Pipeline};
    use std::sync::atomic::{AtomicU64, Ordering};

    let pipeline = Pipeline::new();
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir()
        .join("miri_headless_runner")
        .join(format!("test_{}_{}", std::process::id(), seq));
    fs::create_dir_all(&dir).expect("create test dir");

    let opts = BuildOptions {
        target: BuildTarget::WebGpu,
        out_path: Some(dir.clone()),
        release: false,
        opt_level: 0,
        cpu_backend: Default::default(),
        // Bundle-only: this smoke validates the emitted JS, not a native link.
        emit_native_host: false,
    };
    pipeline.build(source, &opts).expect("build should succeed");
    dir
}

/// The `a[i] + b[i]` vector-add demo. Its `dst` paint buffer reads back
/// `[6, 8, 10, 12]` after a successful dispatch.
const VECTOR_ADD: &str = r#"
use system.gpu

gpu let a = [1, 2, 3, 4]
gpu let b = [5, 6, 7, 8]
gpu var dst = [0, 0, 0, 0]

gpu forall i in 0..4
    dst[i] = a[i] + b[i]
"#;

/// Manifest file emitted for a bundle directory (`<dirname>.json`).
fn manifest_path(bundle_dir: &std::path::Path) -> PathBuf {
    let name = bundle_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("bundle");
    bundle_dir.join(format!("{}.json", name))
}

/// Resolve a JS runtime binary for `env_var` (e.g. `MIRI_NODE`), falling back
/// to `default_name` on `PATH`. Returns None when neither resolves (test skip).
fn resolve_runtime(env_var: &str, default_name: &str) -> Option<PathBuf> {
    if let Ok(path) = std::env::var(env_var) {
        let p = PathBuf::from(&path);
        if p.exists() {
            return Some(p);
        }
    }
    let which = Command::new("which").arg(default_name).output().ok()?;
    if !which.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&which.stdout).trim().to_string();
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

#[test]
fn bundle_emits_headless_runner_and_package_json() {
    let bundle = build_bundle(VECTOR_ADD);

    let runner = bundle.join("miri-gpu-headless.js");
    let runner_src = fs::read_to_string(&runner).expect("headless runner must be emitted");
    assert!(!runner_src.is_empty(), "headless runner must be non-empty");
    assert!(
        runner_src.contains("runHeadless"),
        "runner must drive runHeadless"
    );
    assert!(
        runner_src.contains("./miri-gpu.js"),
        "runner must import the sibling harness module"
    );

    let pkg = bundle.join("package.json");
    let pkg_src = fs::read_to_string(&pkg).expect("package.json must be emitted");
    assert!(
        pkg_src.contains("\"type\": \"module\""),
        "package.json must mark the bundle as an ES module so `.js` imports as ESM"
    );
}

#[test]
fn harness_exports_run_headless() {
    let bundle = build_bundle(VECTOR_ADD);
    let harness = fs::read_to_string(bundle.join("miri-gpu.js")).expect("harness emitted");
    assert!(
        harness.contains("export async function runHeadless"),
        "miri-gpu.js must export the headless entry point"
    );
}

/// Boot the emitted bundle under Node. Node has no WebGPU, so the run boots
/// through module import + manifest parse + buffer construction and then fails
/// at the device request with a clear `WebGPU unavailable` error. Any *other*
/// failure (a syntax error, a failed import, a manifest-parse crash) means the
/// bundle did not boot and fails the test. Under a WebGPU-enabled Node, the run
/// succeeds and returns the readback values instead.
#[test]
fn headless_runner_boots_bundle_under_node() {
    let node = match resolve_runtime("MIRI_NODE", "node") {
        Some(n) => n,
        None => {
            eprintln!("skipping: no Node runtime found (set MIRI_NODE or install node)");
            return;
        }
    };

    let bundle = build_bundle(VECTOR_ADD);
    let runner = bundle.join("miri-gpu-headless.js");
    let manifest = manifest_path(&bundle);

    let output = Command::new(&node)
        .arg(&runner)
        .arg(&manifest)
        .output()
        .expect("run node headless runner");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.success() {
        // WebGPU-enabled runtime: the bundle dispatched and read back.
        let result: serde_json::Value =
            serde_json::from_str(stdout.trim()).expect("runner must print JSON on success");
        let values = result["values"].as_array().expect("values array");
        let got: Vec<i64> = values
            .iter()
            .map(|v| v.as_i64().unwrap_or_default())
            .collect();
        assert_eq!(got, vec![6, 8, 10, 12], "vector-add readback");
    } else {
        // GPU-less runtime: booted to the device request, then a clean stop.
        assert!(
            stderr.contains("WebGPU unavailable"),
            "bundle must boot to the WebGPU device request; unexpected failure:\nstdout: {}\nstderr: {}",
            stdout,
            stderr
        );
    }
}

/// Strict positive smoke under Deno (a WebGPU-capable runtime). Skips when Deno
/// is absent; when present, the bundle must dispatch and return the readback.
#[test]
fn headless_runner_dispatches_under_deno() {
    let deno = match resolve_runtime("MIRI_DENO", "deno") {
        Some(d) => d,
        None => {
            eprintln!("skipping: no Deno runtime found (set MIRI_DENO or install deno)");
            return;
        }
    };

    let bundle = build_bundle(VECTOR_ADD);
    let runner = bundle.join("miri-gpu-headless.js");
    let manifest = manifest_path(&bundle);

    let output = Command::new(&deno)
        .arg("run")
        .arg("--allow-read")
        .arg("--unstable-webgpu")
        .arg(&runner)
        .arg(&manifest)
        .output()
        .expect("run deno headless runner");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "deno headless run must dispatch and exit 0:\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );
    let result: serde_json::Value =
        serde_json::from_str(stdout.trim()).expect("runner must print JSON on success");
    let values = result["values"].as_array().expect("values array");
    let got: Vec<i64> = values
        .iter()
        .map(|v| v.as_i64().unwrap_or_default())
        .collect();
    assert_eq!(got, vec![6, 8, 10, 12], "vector-add readback under Deno");
}
