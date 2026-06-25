// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Browser-class WGSL validation gate using Tint (Chrome's WGSL compiler).
//!
//! Unlike naga (wgpu's validator), Tint enforces strict WebGPU spec compliance,
//! catching language features that browsers reject:
//! - No `i64` / `u64` / `f64` scalar types (naga-permissive, Tint-rejecting)
//! - No `__` identifier prefix (reserved in WebGPU)
//! - No unsupported `enable` directives
//!
//! The validation harness resolves the tint binary in order:
//! 1. Environment variable `MIRI_TINT`
//! 2. Repository path `tools/tint/tint`
//! 3. System `PATH` via `which tint`
//!
//! Tests:
//! - Plumbing tests use a fake-tint stub (checks 64-bit scalar types only; always-run, deterministic).
//!   Full coverage (reserved prefixes, enable directives) is the real-tint job.
//! - Real-tint gate is feature-gated (`browser-gpu-gate`) for CI only

use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Helper to build a demo program and return bundle directory path.
fn build_demo_manifest(demo_path: &str) -> serde_json::Value {
    use miri::codegen::backend::BuildTarget;
    use miri::pipeline::{BuildOptions, Pipeline};
    use std::sync::atomic::{AtomicU64, Ordering};

    let source = fs::read_to_string(demo_path)
        .unwrap_or_else(|_| panic!("Failed to read demo file: {}", demo_path));

    let pipeline = Pipeline::new();
    static BUNDLE_DIR_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = BUNDLE_DIR_SEQ.fetch_add(1, Ordering::Relaxed);
    let temp_base = std::env::temp_dir().join("miri_browser_gate").join(format!(
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

    let _index_html_path = pipeline
        .build(&source, &opts)
        .unwrap_or_else(|e| panic!("Failed to build {}: {}", demo_path, e));

    // Read the manifest JSON from the bundle directory
    let dir_name = temp_base
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("bundle");
    let manifest_path = temp_base.join(format!("{}.json", dir_name));
    let manifest_text = fs::read_to_string(&manifest_path).expect("Failed to read manifest JSON");
    serde_json::from_str(&manifest_text).expect("Failed to parse manifest JSON")
}

/// Extract all WGSL kernels from a manifest.
///
/// Returns a vec of (entryPoint, wgsl_source) tuples from:
/// - `manifest["seed"][].wgsl` — compute kernels
/// - `manifest["frame"].wgsl` — optional single frame step kernel
/// - `manifest["framePasses"][].wgsl` — ordered passes of a multi-pass frame
///
/// Does not panic if any section is missing.
fn extract_kernels(manifest: &serde_json::Value) -> Vec<(String, String)> {
    let mut kernels = Vec::new();

    push_kernel_array(&manifest["seed"], &mut kernels);
    push_kernel_array(&manifest["framePasses"], &mut kernels);

    // Extract single frame kernel if present
    if let Some(frame) = manifest["frame"].as_object() {
        if let Some(entry_point) = frame.get("entryPoint").and_then(|v| v.as_str()) {
            if let Some(wgsl) = frame.get("wgsl").and_then(|v| v.as_str()) {
                kernels.push((entry_point.to_string(), wgsl.to_string()));
            }
        }
    }

    kernels
}

/// Push every `{entryPoint, wgsl}` entry of a manifest kernel array onto `out`.
fn push_kernel_array(value: &serde_json::Value, out: &mut Vec<(String, String)>) {
    if let Some(array) = value.as_array() {
        for kernel in array {
            if let (Some(entry_point), Some(wgsl)) =
                (kernel["entryPoint"].as_str(), kernel["wgsl"].as_str())
            {
                out.push((entry_point.to_string(), wgsl.to_string()));
            }
        }
    }
}

/// Collect every `.mi` demo under `examples/gpu`, including the interactive
/// `web/` bundles. Both the seed-and-frame native demos and the web demos go
/// through the same browser-class validation matrix.
fn collect_demo_files() -> Vec<PathBuf> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("gpu");
    let mut files = collect_mi_files(&base);
    files.extend(collect_mi_files(&base.join("web")));
    files.sort();
    files
}

/// Collect the `.mi` files directly inside `dir` (non-recursive).
fn collect_mi_files(dir: &std::path::Path) -> Vec<PathBuf> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };
    entries
        .filter_map(|entry| {
            entry.ok().and_then(|e| {
                let path = e.path();
                if path.extension().map_or(false, |ext| ext == "mi") {
                    Some(path)
                } else {
                    None
                }
            })
        })
        .collect()
}

/// Resolve the tint binary location.
///
/// Searches in order:
/// 1. `MIRI_TINT` environment variable
/// 2. Repository path `tools/tint/tint`
/// 3. System `PATH` via which-like lookup
///
/// Returns None if tint cannot be found.
#[cfg_attr(not(feature = "browser-gpu-gate"), allow(dead_code))]
fn resolve_tint() -> Option<PathBuf> {
    resolve_tint_from(std::env::var("MIRI_TINT").ok())
}

/// Resolution logic with the `MIRI_TINT` value injected explicitly, so tests
/// can exercise it without mutating the process-global environment (which would
/// race the concurrently-running real-gate test that reads `MIRI_TINT`).
fn resolve_tint_from(miri_tint: Option<String>) -> Option<PathBuf> {
    // 1. Explicit override
    if let Some(tint_path) = miri_tint {
        let path = PathBuf::from(&tint_path);
        if path.exists() {
            return Some(path);
        }
    }

    // 2. Check repository path
    let repo_tint = PathBuf::from("tools/tint/tint");
    if repo_tint.exists() {
        return Some(repo_tint);
    }

    // 3. Try to find in PATH
    if let Ok(output) = Command::new("which").arg("tint").output() {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path_str.is_empty() {
                return Some(PathBuf::from(path_str));
            }
        }
    }

    None
}

/// Run tint validation on a WGSL source string.
///
/// Writes the WGSL to a temporary file and invokes tint to validate it.
/// Returns Ok(()) on valid WGSL, Err(tint stderr output) on invalid.
///
/// Tint invocation: `tint <wgsl_file> -o <temp_file>.spv`
/// (pinned output format via .spv extension; exit code signals validity)
fn tint_validate(tint: &std::path::Path, wgsl: &str) -> Result<(), String> {
    use std::io::Write;

    let temp_dir = std::env::temp_dir().join("miri_tint_validate");
    fs::create_dir_all(&temp_dir).ok();

    // The `.wgsl` suffix is required: tint infers the input language from the
    // file extension, and rejects an extensionless temp file with
    // "Unknown input format: unknown" before it ever parses the WGSL.
    let mut temp_file = tempfile::Builder::new()
        .suffix(".wgsl")
        .tempfile_in(&temp_dir)
        .map_err(|e| format!("Failed to create temp file: {}", e))?;
    temp_file
        .write_all(wgsl.as_bytes())
        .map_err(|e| format!("Failed to write WGSL to temp file: {}", e))?;

    let temp_path = temp_file.path().to_path_buf();

    // Write output to a temp file with .spv extension so tint infers format correctly.
    let out_path = temp_dir.join(format!("out_{}.spv", std::process::id()));

    let output = Command::new(tint)
        .arg(&temp_path)
        .arg("-o")
        .arg(&out_path)
        .output()
        .map_err(|e| format!("Failed to run tint: {}", e))?;

    // Clean up output file after validation.
    let _ = fs::remove_file(&out_path);

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        Err(format!(
            "tint validation failed:\nstderr: {}\nstdout: {}",
            stderr, stdout
        ))
    }
}

/// Naga-validate a WGSL kernel; panics with the demo context on failure.
fn naga_validate_demo(demo_name: &str, entry_point: &str, wgsl: &str) {
    let module = naga::front::wgsl::parse_str(wgsl).unwrap_or_else(|err| {
        panic!(
            "Demo {} kernel {} failed naga parse: {}\nWGSL:\n{}",
            demo_name,
            entry_point,
            err.emit_to_string(wgsl),
            wgsl
        )
    });
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    validator.validate(&module).unwrap_or_else(|err| {
        panic!(
            "Demo {} kernel {} failed naga validate: {:?}\nWGSL:\n{}",
            demo_name, entry_point, err, wgsl
        )
    });
}

/// Every demo kernel — including each pass of a multi-pass web demo — must pass
/// naga validation. This is the browser-independent gate that runs in normal
/// CI; the real-tint job (`browser-gpu-gate`) adds the stricter WebGPU layer.
#[test]
fn every_demo_kernel_passes_naga() {
    let demo_files = collect_demo_files();
    assert!(
        !demo_files.is_empty(),
        "Should have at least one .mi demo file in examples/gpu/"
    );

    for demo_path in demo_files {
        let demo_name = demo_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let manifest = build_demo_manifest(demo_path.to_str().unwrap());
        let kernels = extract_kernels(&manifest);
        assert!(
            !kernels.is_empty(),
            "Demo {} should emit at least one kernel",
            demo_name
        );

        for (entry_point, wgsl) in kernels {
            naga_validate_demo(demo_name, &entry_point, &wgsl);
            // 64-bit scalars never reach a browser bundle (no f64/i64/u64 in
            // core WGSL); the device path narrows every kernel value to 32-bit.
            assert!(
                !wgsl.contains("f64"),
                "Demo {} kernel {} must not emit f64 (browser WGSL has no f64)",
                demo_name,
                entry_point
            );
        }
    }
}

#[test]
fn every_demo_emits_at_least_one_kernel() {
    let demo_files = collect_demo_files();

    assert!(
        !demo_files.is_empty(),
        "Should have at least one .mi demo file in examples/gpu/"
    );

    for demo_path in demo_files {
        let demo_name = demo_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let manifest = build_demo_manifest(demo_path.to_str().unwrap());
        let kernels = extract_kernels(&manifest);

        assert!(
            !kernels.is_empty(),
            "Demo {} should emit at least one kernel",
            demo_name
        );

        for (entry_point, wgsl) in kernels {
            assert!(
                !wgsl.is_empty(),
                "Kernel {} in {} should have non-empty WGSL",
                entry_point,
                demo_name
            );
            assert!(
                wgsl.contains("@compute"),
                "Kernel {} in {} should contain @compute attribute",
                entry_point,
                demo_name
            );
        }
    }
}

#[test]
fn tint_driver_accepts_valid_and_rejects_invalid() {
    // Resolve the fake-tint stub
    let stub_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("fake_tint.sh");

    if !stub_path.exists() {
        panic!(
            "Fake tint stub not found at {}. Cannot run tint_driver test.",
            stub_path.display()
        );
    }

    // Test 1: Valid WGSL (no i64)
    let valid_wgsl = r#"
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let x: i32 = i32(gid.x);
}
"#;

    match tint_validate(&stub_path, valid_wgsl) {
        Ok(()) => {} // Expected
        Err(e) => panic!("Valid WGSL should pass validation, but got error: {}", e),
    }

    // Test 2: Invalid WGSL (contains i64 marker)
    let invalid_i64_wgsl = r#"
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let x: i64 = 42i64;
}
"#;

    match tint_validate(&stub_path, invalid_i64_wgsl) {
        Ok(()) => panic!("Invalid WGSL containing i64 should fail validation"),
        Err(msg) => {
            // Expected
            assert!(
                !msg.is_empty(),
                "Error message should be captured from tint"
            );
        }
    }

    // Test 3: Invalid WGSL (contains u64 marker)
    let invalid_u64_wgsl = r#"
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let y: u64 = 100u64;
}
"#;

    match tint_validate(&stub_path, invalid_u64_wgsl) {
        Ok(()) => panic!("Invalid WGSL containing u64 should fail validation"),
        Err(msg) => {
            // Expected
            assert!(
                !msg.is_empty(),
                "Error message should be captured from tint"
            );
        }
    }

    // Test 4: Invalid WGSL (contains f64 marker)
    let invalid_f64_wgsl = r#"
@compute @workgroup_size(8, 8, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let z: f64 = 3.14f64;
}
"#;

    match tint_validate(&stub_path, invalid_f64_wgsl) {
        Ok(()) => panic!("Invalid WGSL containing f64 should fail validation"),
        Err(msg) => {
            // Expected
            assert!(
                !msg.is_empty(),
                "Error message should be captured from tint"
            );
        }
    }
}

#[test]
fn missing_tint_is_a_loud_error() {
    // A nonexistent MIRI_TINT path must not resolve to that path. Injected
    // explicitly — no global env mutation, so this cannot race the real-gate
    // test. (Holds as long as `tools/tint/tint` is absent and `tint` is not on
    // PATH, which is the case in the gate's CI job and a clean checkout.)
    assert!(
        resolve_tint_from(Some("/nonexistent/fake/tint".to_string())).is_none(),
        "resolve_tint_from should fail when the override path does not exist"
    );
}

/// Real-tint validation test (feature-gated for CI only).
/// This test requires the real tint binary to be available.
#[cfg(feature = "browser-gpu-gate")]
#[test]
fn all_demo_kernels_pass_tint() {
    let tint_path = resolve_tint().unwrap_or_else(|| {
        panic!(
            "browser-gpu-gate feature enabled but tint not found.\n\
             To obtain tint, build from Dawn:\n\
             - Clone Dawn at a pinned revision\n\
             - Run: cmake -DTINT_BUILD_CMD_TOOLS=ON ... && cmake --build . -t tint\n\
             - Set MIRI_TINT=/path/to/tint or ensure tint is on PATH\n\
             - Rerun: cargo test --features browser-gpu-gate --test mod browser_validation"
        )
    });

    let demo_files = collect_demo_files();

    let mut all_errors = Vec::new();

    for demo_path in demo_files {
        let demo_name = demo_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let manifest = build_demo_manifest(demo_path.to_str().unwrap());
        let kernels = extract_kernels(&manifest);

        for (entry_point, wgsl) in kernels {
            if let Err(tint_error) = tint_validate(&tint_path, &wgsl) {
                all_errors.push(format!(
                    "Demo: {}\n  Kernel: {}\n  Error:\n    {}\n  WGSL:\n{}",
                    demo_name, entry_point, tint_error, wgsl
                ));
            }
        }
    }

    if !all_errors.is_empty() {
        panic!(
            "Some kernels failed tint validation:\n\n{}",
            all_errors.join("\n\n")
        );
    }
}
