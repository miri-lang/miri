// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! HTML harness emitted by `miri build --target web-gpu`.
//!
//! Produces a single self-contained page that:
//! 1. Imports the JS runtime shim emitted alongside it.
//! 2. Fetches each kernel's WGSL text from `kernels/<entry>.wgsl`.
//! 3. Dispatches each kernel against a host-zeroed storage buffer (so the
//!    page renders without requiring callers to author per-kernel inputs).
//! 4. Reads back the first kernel's output buffer and renders it as a 2D
//!    pixel field on the bundled `<canvas>` element.
//!
//! This is a starter harness — author programs can replace it after running
//! `miri build` once. The goal is "open in a WebGPU browser, see something
//! computed by the program," not a finished demo authoring API.

use super::KernelArtifact;

/// Render the bundle's `index.html` text from the compiled kernel artifacts.
pub(super) fn render(kernels: &[KernelArtifact]) -> String {
    let manifest = render_manifest(kernels);
    HTML_TEMPLATE
        .replace(KERNEL_MANIFEST_PLACEHOLDER, &manifest)
        .replace(BUNDLE_TITLE_PLACEHOLDER, BUNDLE_TITLE)
}

const KERNEL_MANIFEST_PLACEHOLDER: &str = "/*__MIRI_KERNEL_MANIFEST__*/";
const BUNDLE_TITLE_PLACEHOLDER: &str = "__MIRI_BUNDLE_TITLE__";
const BUNDLE_TITLE: &str = "Miri WebGPU bundle";

fn render_manifest(kernels: &[KernelArtifact]) -> String {
    let mut out = String::from("[\n");
    for kernel in kernels {
        let entry = escape_js_string(&kernel.entry_point);
        let file = escape_js_string(&kernel.file_name);
        let [wx, wy, wz] = kernel.workgroup_size;
        out.push_str(&format!(
            "    {{ entryPoint: \"{entry}\", wgslPath: \"kernels/{file}\", workgroupSize: [{wx}, {wy}, {wz}] }},\n"
        ));
    }
    out.push(']');
    out
}

fn escape_js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

const HTML_TEMPLATE: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8" />
    <title>__MIRI_BUNDLE_TITLE__</title>
    <style>
        body { font-family: -apple-system, system-ui, sans-serif; max-width: 64rem; margin: 2rem auto; padding: 0 1rem; }
        h1 { margin-bottom: 0.25rem; }
        canvas { display: block; margin: 1rem 0; background: #111; image-rendering: pixelated; width: 256px; height: 256px; }
        #log { font-family: ui-monospace, SFMono-Regular, Consolas, monospace; font-size: 0.85rem; background: #f5f5f5; border: 1px solid #ddd; border-radius: 4px; padding: 0.75rem; white-space: pre-wrap; }
        .pass { color: #0a7d28; font-weight: 600; }
        .fail { color: #b00020; font-weight: 600; }
    </style>
</head>
<body>
    <h1>__MIRI_BUNDLE_TITLE__</h1>
    <p>Compute results rendered from <code>gpu fn</code> kernels emitted by <code>miri build --target web-gpu</code>. Open in a WebGPU-capable browser.</p>
    <canvas id="output" width="64" height="64" aria-label="Compute output pixel grid"></canvas>
    <div id="status">Booting…</div>
    <pre id="log"></pre>

    <script type="module">
        import { initGpu, runKernel, MiriGpuError } from "./miri_gpu_runtime.js";

        const KERNELS = /*__MIRI_KERNEL_MANIFEST__*/;
        const DEFAULT_BUFFER_LENGTH = 4096;
        const CANVAS_SIDE = 64;

        const status = document.getElementById("status");
        const log = document.getElementById("log");
        const canvas = document.getElementById("output");

        function logLine(msg) {
            log.textContent += msg + "\n";
        }

        async function fetchWgsl(path) {
            const response = await fetch(path);
            if (!response.ok) {
                throw new MiriGpuError(`failed to fetch ${path}: ${response.status} ${response.statusText}`);
            }
            return await response.text();
        }

        function paintCanvas(values) {
            const ctx = canvas.getContext("2d");
            const width = canvas.width;
            const height = canvas.height;
            const image = ctx.createImageData(width, height);
            const total = width * height;
            let min = Number.POSITIVE_INFINITY;
            let max = Number.NEGATIVE_INFINITY;
            for (let i = 0; i < Math.min(total, values.length); i++) {
                const v = Number(values[i]);
                if (Number.isFinite(v)) {
                    if (v < min) min = v;
                    if (v > max) max = v;
                }
            }
            const range = max > min ? max - min : 1;
            for (let i = 0; i < total; i++) {
                const raw = i < values.length ? Number(values[i]) : 0;
                const normalized = Number.isFinite(raw) ? (raw - min) / range : 0;
                const intensity = Math.max(0, Math.min(255, Math.round(normalized * 255)));
                const off = i * 4;
                image.data[off + 0] = intensity;
                image.data[off + 1] = intensity;
                image.data[off + 2] = intensity;
                image.data[off + 3] = 255;
            }
            ctx.putImageData(image, 0, 0);
        }

        async function runBundle() {
            if (!KERNELS.length) {
                status.textContent = "No kernels in bundle.";
                status.classList.add("fail");
                return;
            }
            let device;
            try {
                ({ device } = await initGpu());
            } catch (err) {
                status.textContent = `GPU init failed: ${err.message ?? err}`;
                status.classList.add("fail");
                return;
            }

            let firstOutput = null;
            for (const kernel of KERNELS) {
                try {
                    const wgsl = await fetchWgsl(kernel.wgslPath);
                    const length = DEFAULT_BUFFER_LENGTH;
                    const blockSize = kernel.workgroupSize[0] || 1;
                    const workgroups = Math.max(1, Math.ceil(length / blockSize));
                    const [out] = await runKernel({
                        device,
                        wgsl,
                        entryPoint: kernel.entryPoint,
                        inputs: [],
                        outputs: [
                            {
                                length,
                                type: Int32Array,
                                initialData: new Int32Array(length),
                            },
                        ],
                        workgroups,
                    });
                    logLine(`${kernel.entryPoint}: ran, first 4 = [${out[0]}, ${out[1]}, ${out[2]}, ${out[3]}]`);
                    if (firstOutput === null) firstOutput = out;
                } catch (err) {
                    logLine(`${kernel.entryPoint}: FAIL ${err.message ?? err}`);
                }
            }

            if (firstOutput) {
                paintCanvas(firstOutput);
                status.textContent = `Rendered ${KERNELS.length} kernel(s). First output painted on canvas.`;
                status.classList.add("pass");
            } else {
                status.textContent = "No kernel produced output.";
                status.classList.add("fail");
            }
        }

        runBundle();
    </script>
</body>
</html>
"##;
