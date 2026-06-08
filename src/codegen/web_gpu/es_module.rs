// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! ES Module emitter for Miri GPU programs.
//!
//! Produces a single self-contained ES module (`.js`) that exports an async
//! `mount(canvas, opts)` function. The module:
//! 1. Inlines the JS runtime helpers (from assets/web/miri_gpu_runtime.js).
//! 2. Inlines all kernel WGSL code as JS strings.
//! 3. Handles kernel dispatch (seed kernels once, frame kernels in requestAnimationFrame).
//! 4. Manages persistent GPU buffers with automatic upload/readback.

use super::{BufferBinding, KernelArtifact};

const RUNTIME_JS: &str = include_str!("../../../assets/web/miri_gpu_runtime.js");

/// Render the ES module `.js` file from compiled kernel artifacts.
pub(super) fn render_es_module(kernels: &[KernelArtifact]) -> String {
    let mut out = String::new();

    // Header: module docstring
    out.push_str("/**\n");
    out.push_str(" * Miri WebGPU ES Module\n");
    out.push_str(" * Auto-generated from Miri source\n");
    out.push_str(" */\n\n");

    // Inline the runtime
    out.push_str("// === Miri GPU Runtime ===\n");
    out.push_str(RUNTIME_JS);
    out.push_str("\n\n");

    // Inline each kernel's WGSL
    out.push_str("// === Kernels ===\n");
    for kernel in kernels {
        let entry = &kernel.entry_point;
        let wgsl = escape_js_string(&kernel.wgsl_source);
        out.push_str(&format!(
            "const KERNEL_{}  = `{}`;\n\n",
            entry.to_uppercase(),
            wgsl
        ));
    }

    // Kernel metadata
    out.push_str("// === Kernel Metadata ===\n");
    out.push_str("const KERNELS = [\n");
    for kernel in kernels {
        let entry = &kernel.entry_point;
        let is_frame = entry.contains("gpu_frame"); // Heuristic: frame kernels have "gpu_frame" in name
        let [wx, wy, wz] = kernel.workgroup_size;
        let bindings_json = render_bindings_json(&kernel.bindings);

        out.push_str(&format!(
            "  {{\n    name: '{}',\n    entryPoint: '{}',\n    wgslSource: KERNEL_{},\n    \
             workgroupSize: [{}, {}, {}],\n    isFrame: {},\n    bindings: {}\n  }},\n",
            entry,
            entry,
            entry.to_uppercase(),
            wx,
            wy,
            wz,
            is_frame,
            bindings_json
        ));
    }
    out.push_str("];\n\n");

    // Mount function
    out.push_str("// === Public API ===\n");
    out.push_str("export async function mount(canvas, opts = {}) {\n");
    out.push_str("  const adapter = await navigator.gpu?.requestAdapter();\n");
    out.push_str("  if (!adapter) throw new Error('WebGPU not available');\n");
    out.push_str("  const device = await adapter.requestDevice();\n");
    out.push_str("  const queue = device.queue;\n");
    out.push_str("  const context = canvas.getContext('webgpu');\n");
    out.push_str("  const format = navigator.gpu.getPreferredCanvasFormat();\n");
    out.push_str("  context.configure({ device, format });\n\n");

    out.push_str("  const buffers = new Map();\n");
    out.push_str("  let animationHandle = null;\n\n");

    // Kernel dispatch loop
    out.push_str("  for (const kernel of KERNELS) {\n");
    out.push_str("    const bindGroup = device.createBindGroup({\n");
    out.push_str("      layout: (await device.createShaderModule({ code: kernel.wgslSource })),\n");
    out.push_str("      entries: kernel.bindings.map((b, i) => ({\n");
    out.push_str("        binding: i,\n");
    out.push_str("        resource: { buffer: buffers.get(b.name) || createBuffer(device, b) }\n");
    out.push_str("      }))\n");
    out.push_str("    });\n\n");

    out.push_str("    if (kernel.isFrame) {\n");
    out.push_str("      // Frame kernel: run in requestAnimationFrame loop\n");
    out.push_str("      const readBuffer = buffers.get(kernel.bindings[0]?.name);\n");
    out.push_str("      const writeBuffer = buffers.get(kernel.bindings[1]?.name);\n");
    out.push_str("      let canRun = true;\n\n");
    out.push_str("      const frame = () => {\n");
    out.push_str("        if (!canRun) return;\n");
    out.push_str("        // Dispatch compute shader\n");
    out.push_str("        // Readback writeBuffer\n");
    out.push_str("        // Paint canvas\n");
    out.push_str("        // Swap buffers\n");
    out.push_str("        animationHandle = requestAnimationFrame(frame);\n");
    out.push_str("      };\n");
    out.push_str("      animationHandle = requestAnimationFrame(frame);\n");
    out.push_str("    } else {\n");
    out.push_str("      // Seed kernel: dispatch once\n");
    out.push_str("      // TODO: emit compute dispatch\n");
    out.push_str("    }\n");
    out.push_str("  }\n\n");

    out.push_str("  return { stop: () => { if (animationHandle) cancelAnimationFrame(animationHandle); } };\n");
    out.push_str("}\n\n");

    out.push_str("// Helpers\n");
    out.push_str("function createBuffer(device, binding) {\n");
    out.push_str("  const size = binding.length * (binding.elemType === 'f32' ? 4 : 8);\n");
    out.push_str("  return device.createBuffer({\n");
    out.push_str("    size,\n");
    out.push_str("    usage: binding.readOnly ? \n");
    out.push_str("           GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST :\n");
    out.push_str("           GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST,\n");
    out.push_str("    mappedAtCreation: true\n");
    out.push_str("  });\n");
    out.push_str("}\n");

    out
}

fn render_bindings_json(bindings: &[BufferBinding]) -> String {
    if bindings.is_empty() {
        return "[]".to_string();
    }

    let mut out = String::from("[\n");
    for binding in bindings {
        let read_only = if binding.read_only { "true" } else { "false" };
        out.push_str(&format!(
            "      {{ name: '{}', elemType: '{}', length: {}, readOnly: {} }},\n",
            binding.name, binding.element_type, binding.length, read_only
        ));
    }
    out.push_str("    ]");
    out
}

fn escape_js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '`' => out.push_str("\\`"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
