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
pub(super) fn render(kernels: &[KernelArtifact], source: Option<&str>) -> String {
    let manifest = render_manifest(kernels);
    let source_panel = source.map(render_source_panel).unwrap_or_default();
    HTML_TEMPLATE
        .replace(KERNEL_MANIFEST_PLACEHOLDER, &manifest)
        .replace(SOURCE_PANEL_PLACEHOLDER, &source_panel)
        .replace(BUNDLE_TITLE_PLACEHOLDER, BUNDLE_TITLE)
}

const KERNEL_MANIFEST_PLACEHOLDER: &str = "/*__MIRI_KERNEL_MANIFEST__*/";
const SOURCE_PANEL_PLACEHOLDER: &str = "<!--__MIRI_SOURCE_PANEL__-->";
const BUNDLE_TITLE_PLACEHOLDER: &str = "__MIRI_BUNDLE_TITLE__";
const BUNDLE_TITLE: &str = "Miri WebGPU bundle";

fn render_manifest(kernels: &[KernelArtifact]) -> String {
    let mut out = String::from("[\n");
    for kernel in kernels {
        let entry = escape_js_string(&kernel.entry_point);
        let file = escape_js_string(&kernel.file_name);
        let [wx, wy, wz] = kernel.workgroup_size;
        let bindings_str = render_kernel_bindings(&kernel.bindings);

        out.push_str(&format!(
            "    {{ entryPoint: \"{entry}\", wgslPath: \"kernels/{file}\", workgroupSize: [{wx}, {wy}, {wz}]{} }},\n",
            bindings_str
        ));
    }
    out.push(']');
    out
}

fn render_kernel_bindings(bindings: &[super::BufferBinding]) -> String {
    if bindings.is_empty() {
        return String::new();
    }

    let mut out = String::from(", bindings: {\n");
    for binding in bindings {
        let name = escape_js_string(&binding.name);
        let elem_type = escape_js_string(&binding.element_type);
        let read_only = if binding.read_only { "true" } else { "false" };
        let length = binding.length;
        let initial_data_str = render_initial_data(&binding.initial_data, length);

        out.push_str(&format!(
            "      \"{name}\": {{ elemType: \"{elem_type}\", length: {length}, readOnly: {read_only}, initialData: {initial_data_str} }},\n"
        ));
    }
    out.push_str("    }");
    out
}

fn render_initial_data(values: &[f64], length: usize) -> String {
    if values.is_empty() {
        format!("new Array({})", length)
    } else {
        format!(
            "[{}]",
            values
                .iter()
                .map(|v| {
                    if v.fract() == 0.0 && *v >= 0.0 && *v < i32::MAX as f64 {
                        format!("{}", *v as i32)
                    } else {
                        format!("{}", v)
                    }
                })
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
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

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn render_source_panel(source: &str) -> String {
    let escaped = escape_html(source);
    format!(
        r#"<div id="sourcePanel" style="margin-top: 2rem; padding: 1rem; background: #f9f9f9; border: 1px solid #ddd; border-radius: 4px;">
    <h2 style="margin-top: 0;">Source Code</h2>
    <pre style="background: #fff; padding: 0.75rem; border: 1px solid #e0e0e0; border-radius: 3px; overflow-x: auto; font-size: 0.85rem;">{}</pre>
</div>"#,
        escaped
    )
}

const HTML_TEMPLATE: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8" />
    <title>__MIRI_BUNDLE_TITLE__</title>
    <style>
        body { font-family: -apple-system, system-ui, sans-serif; max-width: 64rem; margin: 2rem auto; padding: 0 1rem; }
        h1 { margin-bottom: 0.25rem; }
        #layout { display: grid; grid-template-columns: 1fr 1fr; gap: 2rem; margin: 1rem 0; }
        #renderPanel { }
        #sourcePanel { background: #f9f9f9; border: 1px solid #ddd; border-radius: 4px; padding: 1rem; }
        canvas { display: block; margin: 1rem 0; background: #111; image-rendering: pixelated; width: 256px; height: 256px; }
        pre { background: #fff; padding: 0.75rem; border: 1px solid #e0e0e0; border-radius: 3px; overflow-x: auto; font-size: 0.85rem; font-family: ui-monospace, SFMono-Regular, Consolas, monospace; }
        #log { font-family: ui-monospace, SFMono-Regular, Consolas, monospace; font-size: 0.85rem; background: #f5f5f5; border: 1px solid #ddd; border-radius: 4px; padding: 0.75rem; white-space: pre-wrap; max-height: 300px; overflow-y: auto; }
        .pass { color: #0a7d28; font-weight: 600; }
        .fail { color: #b00020; font-weight: 600; }
        @media (max-width: 900px) {
            #layout { grid-template-columns: 1fr; }
        }
    </style>
</head>
<body>
    <h1>__MIRI_BUNDLE_TITLE__</h1>
    <p>Compute results rendered from GPU kernels emitted by <code>miri build --target web-gpu</code>. Open in a WebGPU-capable browser.</p>
    <div id="layout">
        <div id="renderPanel">
            <canvas id="output" width="64" height="64" aria-label="Compute output pixel grid"></canvas>
            <div id="status">Booting…</div>
            <pre id="log"></pre>
        </div>
        <div>
            <!--__MIRI_SOURCE_PANEL__-->
        </div>
    </div>

    <script type="module">
        import { initGpu, runKernel, MiriGpuError } from "./miri_gpu_runtime.js";

        const KERNELS = /*__MIRI_KERNEL_MANIFEST__*/;
        const ANIMATE = false;
        let animationId = null;

        const status = document.getElementById("status");
        const log = document.getElementById("log");
        const canvas = document.getElementById("output");

        function logLine(msg) {
            log.textContent += msg + "\n";
            log.scrollTop = log.scrollHeight;
        }

        async function fetchWgsl(path) {
            const response = await fetch(path);
            if (!response.ok) {
                throw new MiriGpuError(`failed to fetch ${path}: ${response.status} ${response.statusText}`);
            }
            return await response.text();
        }

        function elemTypeToArrayType(elemType) {
            switch (elemType) {
                case "i32": return Int32Array;
                case "u32": return Uint32Array;
                case "f32": return Float32Array;
                case "i64": return BigInt64Array;
                case "u64": return BigUint64Array;
                case "f64": return Float64Array;
                default: return Int32Array;
            }
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

        let device = null;
        let firstOutput = null;

        async function dispatch() {
            if (!KERNELS.length) {
                status.textContent = "No kernels in bundle.";
                status.classList.add("fail");
                return;
            }

            for (const kernel of KERNELS) {
                try {
                    const wgsl = await fetchWgsl(kernel.wgslPath);
                    const blockSize = kernel.workgroupSize[0] || 1;

                    const inputs = [];
                    const outputs = [];

                    const bindings = kernel.bindings || {};
                    for (const [bufName, bufMeta] of Object.entries(bindings)) {
                        const ArrayType = elemTypeToArrayType(bufMeta.elemType);
                        const buffer = {
                            length: bufMeta.length,
                            type: ArrayType,
                            initialData: bufMeta.initialData.length > 0
                                ? new ArrayType(bufMeta.initialData)
                                : new ArrayType(bufMeta.length),
                        };

                        if (bufMeta.readOnly) {
                            inputs.push(buffer);
                        } else {
                            outputs.push(buffer);
                        }
                    }

                    const workgroups = Math.max(1, Math.ceil((outputs[0]?.length || 1) / blockSize));
                    const [out] = await runKernel({
                        device,
                        wgsl,
                        entryPoint: kernel.entryPoint,
                        inputs,
                        outputs,
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
                if (status.textContent === "Booting…") {
                    status.textContent = `Rendered ${KERNELS.length} kernel(s). First output painted on canvas.`;
                    status.classList.add("pass");
                }
            }
        }

        async function animationLoop() {
            await dispatch();
            if (ANIMATE) {
                animationId = requestAnimationFrame(animationLoop);
            }
        }

        async function runBundle() {
            try {
                ({ device } = await initGpu());
            } catch (err) {
                status.textContent = `GPU init failed: ${err.message ?? err}`;
                status.classList.add("fail");
                return;
            }
            await animationLoop();
        }

        runBundle();
    </script>
</body>
</html>
"##;
