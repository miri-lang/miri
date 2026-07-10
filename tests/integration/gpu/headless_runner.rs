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

/// Extract the WGSL source from a `const NAME = \`…\`;` backtick template in the
/// runtime module. Panics if the marker or its closing backtick is missing.
fn extract_backtick_const(js: &str, name: &str) -> String {
    let marker = format!("const {} = `", name);
    let start = js
        .find(&marker)
        .unwrap_or_else(|| panic!("`{name}` template literal not found in miri-gpu.js"))
        + marker.len();
    let end = js[start..]
        .find("`;")
        .unwrap_or_else(|| panic!("`{name}` template literal is unterminated"));
    js[start..start + end].to_string()
}

/// The runtime ships two hand-written WGSL shaders — the fullscreen present blit
/// and the min/max reduction — that no compiler output covers, so a typo would
/// only surface in a browser. Validate them with `naga` here (parse + validate),
/// exactly as the kernel WGSL is checked.
#[test]
fn runtime_present_and_reduce_wgsl_are_valid() {
    let js = fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/web/miri-gpu.js"
    ))
    .expect("read assets/web/miri-gpu.js");

    for name in ["PRESENT_WGSL", "REDUCE_WGSL"] {
        let wgsl = extract_backtick_const(&js, name);
        let module = naga::front::wgsl::parse_str(&wgsl).unwrap_or_else(|err| {
            panic!(
                "{name}: naga parse failed: {}\nWGSL:\n{wgsl}",
                err.emit_to_string(&wgsl)
            )
        });
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|err| panic!("{name}: naga validate failed: {err:?}\nWGSL:\n{wgsl}"));
    }
}

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

/// A multi-pass animated demo mirroring the interactive-Mandelbrot shape: a
/// `view_a`/`view_b` double-buffered state pair advanced from a `frame.*` input,
/// plus a distinct terminal `paint` output. Exercises the frame-input uniform
/// binding and the double-buffer swap.
const FRAME_PINGPONG: &str = r#"
use system.gpu
use system.collections.array

gpu var view_a = Array<f32, 4>()
gpu var view_b = Array<f32, 4>()
gpu var paint = Array<f32, 16>()

gpu forall i in 0..4
    view_a[i] = 1.0

gpu frame
    gpu forall i in 0..4
        view_b[i] = view_a[i] + frame.wheel
    gpu forall idx in 0..16
        paint[idx] = view_b[0]
"#;

/// A stub-GPU driver (no real WebGPU needed) that imports the emitted
/// `miri-gpu.js`, records the physical buffer bound at each binding across two
/// frames, and prints the trace as JSON. It proves two wirings a GPU-less CI
/// cannot otherwise check: the `frame.*` uniform is bound at the index the WGSL
/// declares, and the state pair double-buffers (frame 1 reads frame 0's write).
const STUB_DRIVER: &str = r#"
import { runHeadless } from "./miri-gpu.js";
import fs from "fs";
globalThis.GPUBufferUsage = { STORAGE:1, COPY_SRC:2, COPY_DST:4, UNIFORM:8, MAP_READ:16 };
globalThis.GPUShaderStage = { COMPUTE:1 };
globalThis.GPUMapMode = { READ:1 };
const dispatches = [];
const readbacks = [];
let pending = null;
function buf(label, size){ const ab = new ArrayBuffer(size||4);
  return { label, getMappedRange:(o=0,l)=>l?ab.slice(o,o+l):ab, unmap(){}, destroy(){}, mapAsync:async()=>{}, size }; }
const device = {
  limits:{}, lost:new Promise(()=>{}),
  createBuffer:({label,size})=>buf(label,size),
  createShaderModule:()=>({}), createBindGroupLayout:()=>({}), createPipelineLayout:()=>({}),
  createComputePipeline:(d)=>({ entry:d.compute.entryPoint }),
  createBindGroup:(d)=>({ entries:d.entries.map(e=>({ binding:e.binding, label:e.resource.buffer.label })) }),
  createCommandEncoder:()=>({
    beginComputePass:()=>({ setPipeline(p){ pending=p&&p.entry; },
      setBindGroup(_,bg){ dispatches.push({ entry:pending, entries:bg.entries }); },
      dispatchWorkgroups(){}, end(){} }),
    copyBufferToBuffer(src){ readbacks.push(src.label); }, finish:()=>({}) }),
  queue:{ submit(){}, writeBuffer(){}, onSubmittedWorkDone:async()=>{} }, destroy(){},
};
Object.defineProperty(globalThis, "navigator",
  { value:{ gpu:{ requestAdapter:async()=>({ requestDevice:async()=>device, limits:{} }) } }, configurable:true });
const manifest = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
await runHeadless(manifest, { frames: 2 });
console.log(JSON.stringify({ paint: manifest.paint, dispatches, readbacks }));
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

/// Under a stub GPU device (any Node, no WebGPU), drive the emitted runtime and
/// assert the two wirings that only manifest at dispatch time: the `frame.*`
/// input uniform is bound at the WGSL-declared binding, and the state pair
/// double-buffers so frame 1 reads the buffer frame 0 wrote. Both regressed to
/// a black canvas before this fix (missing `@binding(2)` layout entry; the
/// terminal paint buffer swapped into the view slot).
#[test]
fn stub_device_binds_inputs_uniform_and_double_buffers() {
    let node = match resolve_runtime("MIRI_NODE", "node") {
        Some(n) => n,
        None => {
            eprintln!("skipping: no Node runtime found (set MIRI_NODE or install node)");
            return;
        }
    };

    let bundle = build_bundle(FRAME_PINGPONG);
    let driver = bundle.join("stub-driver.mjs");
    fs::write(&driver, STUB_DRIVER).expect("write stub driver");
    let manifest = manifest_path(&bundle);

    let output = Command::new(&node)
        .arg(&driver)
        .arg(&manifest)
        .output()
        .expect("run node stub driver");
    assert!(
        output.status.success(),
        "stub driver must run:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let trace: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim())
            .expect("stub driver must print a JSON trace");
    let paint = trace["paint"].as_str().unwrap();
    let dispatches = trace["dispatches"].as_array().unwrap();

    // Every dispatch labels the physical buffer bound at each binding. The frame
    // passes are those following the single seed dispatch.
    let label_at = |d: &serde_json::Value, binding: u64| -> Option<String> {
        d["entries"].as_array().unwrap().iter().find_map(|e| {
            (e["binding"].as_u64() == Some(binding))
                .then(|| e["label"].as_str().unwrap().to_string())
        })
    };

    // The pass that reads `frame.*` must bind an `-inputs` uniform buffer; its
    // binding index is whatever the WGSL declared (not assumed positionally).
    let input_dispatches: Vec<&serde_json::Value> = dispatches
        .iter()
        .filter(|d| {
            d["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["label"].as_str().unwrap().ends_with("-inputs"))
        })
        .collect();
    assert!(
        input_dispatches.len() >= 2,
        "the frame-input pass must dispatch (with its uniform bound) each of the two frames; got {} such dispatches",
        input_dispatches.len()
    );

    // Double-buffer proof: the input pass's read binding (0) alternates physical
    // buffers between frames, and frame 1 reads the buffer frame 0 wrote (1).
    let f0_read = label_at(input_dispatches[0], 0).unwrap();
    let f0_write = label_at(input_dispatches[0], 1).unwrap();
    let f1_read = label_at(input_dispatches[1], 0).unwrap();
    assert_ne!(
        f0_read, f1_read,
        "state pair must swap: frame 1 must not re-read frame 0's source buffer"
    );
    assert_eq!(
        f1_read, f0_write,
        "double-buffer: frame 1 must read the buffer frame 0 wrote ({f0_write})"
    );

    // The terminal paint target is never swapped into the view slot: it is the
    // readback source every frame.
    let readbacks = trace["readbacks"].as_array().unwrap();
    assert!(
        readbacks
            .iter()
            .all(|r| r.as_str().unwrap() == format!("miri-{paint}")),
        "paint must always read back the fixed terminal buffer 'miri-{paint}', got {readbacks:?}"
    );
}

/// A stub-GPU driver for the canvas `mount` path. Stubs a `webgpu` canvas
/// context and requestAnimationFrame, runs two animation frames, and prints —
/// per render pass — the buffer bound at binding 0. Proves the paint buffer is
/// blitted via a render pass each frame (GPU present, no readback).
const MOUNT_STUB_DRIVER: &str = r#"
import { mount } from "./miri-gpu.js";
import fs from "fs";
globalThis.GPUBufferUsage = { STORAGE:1, COPY_SRC:2, COPY_DST:4, UNIFORM:8, MAP_READ:16 };
globalThis.GPUShaderStage = { COMPUTE:1, FRAGMENT:2, VERTEX:4 };
globalThis.GPUMapMode = { READ:1 };

const renderPasses = [];   // binding-0 buffer label per render pass
let renderPipelines = 0;
let pendingBg = null;
function buf(label, size){ const ab = new ArrayBuffer(size||4);
  return { label, getMappedRange:(o=0,l)=>l?ab.slice(o,o+l):ab, unmap(){}, destroy(){}, mapAsync:async()=>{}, size }; }
const device = {
  limits:{}, lost:new Promise(()=>{}),
  createBuffer:({label,size})=>buf(label,size),
  createShaderModule:()=>({}), createBindGroupLayout:()=>({}), createPipelineLayout:()=>({}),
  createComputePipeline:(d)=>({ entry:d.compute.entryPoint }),
  createRenderPipeline:()=>{ renderPipelines++; return {}; },
  createBindGroup:(d)=>({ entries:d.entries.map(e=>({ binding:e.binding, label:e.resource.buffer.label })) }),
  createCommandEncoder:()=>({
    beginComputePass:()=>({ setPipeline(){}, setBindGroup(){}, dispatchWorkgroups(){}, end(){} }),
    beginRenderPass:()=>({ setPipeline(){}, setBindGroup(_,bg){ pendingBg=bg; }, draw(){},
      end(){ const b0 = pendingBg.entries.find(e=>e.binding===0); renderPasses.push(b0 && b0.label); pendingBg=null; } }),
    copyBufferToBuffer(){}, finish:()=>({}) }),
  queue:{ submit(){}, writeBuffer(){}, onSubmittedWorkDone:async()=>{} }, destroy(){},
};
Object.defineProperty(globalThis, "navigator",
  { value:{ gpu:{ requestAdapter:async()=>({ requestDevice:async()=>device, limits:{} }),
    getPreferredCanvasFormat:()=>"bgra8unorm" } }, configurable:true });

let rafCb = null;
globalThis.requestAnimationFrame = (cb) => { rafCb = cb; return 1; };
globalThis.cancelAnimationFrame = () => {};

const ctx = { configure(){}, getCurrentTexture:()=>({ createView:()=>({}) }) };
const canvas = {
  width:0, height:0, getContext:()=>ctx,
  getBoundingClientRect:()=>({ left:0, top:0, width:512, height:512 }),
  addEventListener(){}, removeEventListener(){},
};

const manifest = JSON.parse(fs.readFileSync(process.argv[2], "utf8"));
await mount(canvas, manifest, { onFrame(){} });
let ts = 0;
for (let f = 0; f < 2; f++) { const cb = rafCb; rafCb = null; if (cb) cb(ts); ts += 16; }
console.log(JSON.stringify({ paint: manifest.paint, renderPipelines, renderPasses }));
"#;

/// Under a stub GPU device with a stubbed `webgpu` canvas, drive `mount` for two
/// frames and assert the paint buffer is presented via a render pass each frame
/// (the GPU-present blit) rather than read back to the CPU.
#[test]
fn stub_device_presents_paint_via_render_pass() {
    let node = match resolve_runtime("MIRI_NODE", "node") {
        Some(n) => n,
        None => {
            eprintln!("skipping: no Node runtime found (set MIRI_NODE or install node)");
            return;
        }
    };

    let bundle = build_bundle(FRAME_PINGPONG);
    let driver = bundle.join("mount-stub-driver.mjs");
    fs::write(&driver, MOUNT_STUB_DRIVER).expect("write mount stub driver");
    let manifest = manifest_path(&bundle);

    let output = Command::new(&node)
        .arg(&driver)
        .arg(&manifest)
        .output()
        .expect("run node mount stub driver");
    assert!(
        output.status.success(),
        "mount stub driver must run:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let trace: serde_json::Value =
        serde_json::from_str(String::from_utf8_lossy(&output.stdout).trim())
            .expect("mount stub driver must print JSON");
    let paint = trace["paint"].as_str().unwrap();
    assert_eq!(
        trace["renderPipelines"].as_u64().unwrap(),
        1,
        "mount must build exactly one present render pipeline"
    );
    let passes = trace["renderPasses"].as_array().unwrap();
    assert_eq!(
        passes.len(),
        2,
        "each of the two frames must issue one present render pass"
    );
    assert!(
        passes
            .iter()
            .all(|p| p.as_str() == Some(&format!("miri-{paint}"))),
        "every present render pass must bind the paint buffer 'miri-{paint}' at binding 0, got {passes:?}"
    );
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
