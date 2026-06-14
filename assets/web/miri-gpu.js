// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Miri WebGPU embeddable runtime.
//
// A single, reusable ES module that drives a demo described by a manifest
// emitted by `miri build --target web-gpu`. Integrate it into any page:
//
//   import { mount } from "./miri-gpu.js";
//   import manifest from "./game_of_life.json" assert { type: "json" };
//   const handle = await mount(document.querySelector("#demo"), manifest);
//   // later: handle.stop();
//
// The website owns the layout and shows the .mi source itself; this module only
// computes on the GPU and paints into the canvas you give it.
//
// Manifest schema (produced by the compiler — pure data, no JS):
//   {
//     "name": string,
//     "canvas": { "width": number, "height": number },   // grid dimensions
//     "buffers": [
//        { "name": string, "elemType": "i32"|"u32"|"f32",
//          "length": number, "initialData": number[]|null }
//     ],
//     "seed":  [ Kernel ],          // run once, in order, on mount
//     "framePasses": [ Kernel ],    // run every animation frame (empty = static)
//     "paint": string               // buffer name to paint each frame
//   }
//   Kernel = {
//     "entryPoint": string, "wgsl": string, "workgroups": [number,number,number],
//     "bindings": [ { "name": string, "access": "read"|"read_write" } ],
//     "read":  string|null,         // multi-pass only: first pass's ping-pong source
//     "write": string|null,         // multi-pass only: last pass's ping-pong destination
//     "inputs": [ InputField ]|null // per-frame input uniforms (e.g., frame.*)
//   }

export class MiriGpuError extends Error {
    constructor(message, cause) {
        super(message);
        this.name = "MiriGpuError";
        if (cause !== undefined) this.cause = cause;
    }
}

const TYPED_ARRAYS = {
    i32: Int32Array,
    u32: Uint32Array,
    f32: Float32Array,
};

function typedArrayFor(elemType) {
    const ctor = TYPED_ARRAYS[elemType];
    if (!ctor) {
        throw new MiriGpuError(`unsupported element type '${elemType}' (expected i32/u32/f32)`);
    }
    return ctor;
}

function alignTo4(n) {
    return n % 4 === 0 ? n : n + (4 - (n % 4));
}

async function initGpu(opts) {
    if (typeof navigator === "undefined" || !navigator.gpu) {
        throw new MiriGpuError(
            "WebGPU unavailable: navigator.gpu is undefined. " +
                "Use a WebGPU-capable browser (Chrome/Edge 113+, Safari 18+).",
        );
    }
    const adapter = await navigator.gpu.requestAdapter({
        powerPreference: opts.powerPreference ?? "high-performance",
    });
    if (!adapter) {
        throw new MiriGpuError("requestAdapter() returned null — no GPU available");
    }
    const device = await adapter.requestDevice({ label: "miri-gpu-device" });
    device.lost.then((info) => {
        if (info && info.reason !== "destroyed") {
            console.error(`[miri-gpu] device lost (${info.reason}): ${info.message ?? ""}`);
        }
    });
    return device;
}

const STORAGE_USAGE =
    (typeof GPUBufferUsage !== "undefined" &&
        GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST) ||
    0;

// Allocate one persistent device buffer per manifest buffer, seeded from
// initialData (or zero-filled). These live for the lifetime of the mount.
function createBuffers(device, manifest) {
    const buffers = new Map();
    for (const spec of manifest.buffers) {
        const ArrayType = typedArrayFor(spec.elemType);
        const data = new ArrayType(spec.length);
        if (spec.initialData) data.set(spec.initialData);
        const byteLength = alignTo4(data.byteLength || 4);
        const buffer = device.createBuffer({
            label: `miri-${spec.name}`,
            size: byteLength,
            usage: STORAGE_USAGE,
            mappedAtCreation: true,
        });
        new Uint8Array(buffer.getMappedRange()).set(
            new Uint8Array(data.buffer, 0, data.byteLength),
        );
        buffer.unmap();
        buffers.set(spec.name, {
            buffer,
            elemType: spec.elemType,
            length: spec.length,
            byteLength,
        });
    }
    return buffers;
}

function compilePipeline(device, kernel) {
    const module = device.createShaderModule({ label: kernel.entryPoint, code: kernel.wgsl });
    const layout = device.createBindGroupLayout({
        label: `${kernel.entryPoint}-bgl`,
        entries: kernel.bindings.map((b, i) => ({
            binding: i,
            visibility: GPUShaderStage.COMPUTE,
            buffer: { type: b.access === "read" ? "read-only-storage" : "storage" },
        })),
    });
    const pipeline = device.createComputePipeline({
        label: `${kernel.entryPoint}-pipeline`,
        layout: device.createPipelineLayout({ bindGroupLayouts: [layout] }),
        compute: { module, entryPoint: kernel.entryPoint },
    });
    return { pipeline, layout };
}

// Dispatch one kernel. `resolve(name)` maps a binding name to the GPUBuffer to
// bind there, letting the caller swap physical buffers for ping-pong.
function dispatchKernel(device, compiled, kernel, resolve) {
    const bindGroup = device.createBindGroup({
        label: `${kernel.entryPoint}-bg`,
        layout: compiled.layout,
        entries: kernel.bindings.map((b, i) => ({
            binding: i,
            resource: { buffer: resolve(b.name) },
        })),
    });
    const [gx, gy, gz] = kernel.workgroups;
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginComputePass();
    pass.setPipeline(compiled.pipeline);
    pass.setBindGroup(0, bindGroup);
    pass.dispatchWorkgroups(gx || 1, gy || 1, gz || 1);
    pass.end();
    device.queue.submit([encoder.finish()]);
}

async function readBackInto(device, src, byteLength, ArrayType) {
    const size = alignTo4(byteLength);
    const staging = device.createBuffer({
        label: "miri-readback",
        size,
        usage: GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST,
    });
    const encoder = device.createCommandEncoder();
    encoder.copyBufferToBuffer(src, 0, staging, 0, size);
    device.queue.submit([encoder.finish()]);
    await staging.mapAsync(GPUMapMode.READ, 0, size);
    const view = new ArrayType(staging.getMappedRange(0, byteLength).slice(0));
    staging.unmap();
    staging.destroy();
    return view;
}

// Colormaps map a normalized value t∈[0,1] to [r,g,b] (0-255). Pick one per
// demo via `mount(canvas, manifest, { colormap: "spectrum" })`.
function hsvToRgb(h, s, v) {
    const c = v * s;
    const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
    const m = v - c;
    let r = 0;
    let g = 0;
    let b = 0;
    if (h < 60) [r, g, b] = [c, x, 0];
    else if (h < 120) [r, g, b] = [x, c, 0];
    else if (h < 180) [r, g, b] = [0, c, x];
    else if (h < 240) [r, g, b] = [0, x, c];
    else if (h < 300) [r, g, b] = [x, 0, c];
    else [r, g, b] = [c, 0, x];
    return [(r + m) * 255, (g + m) * 255, (b + m) * 255];
}

const COLORMAPS = {
    // Grayscale: linear black→white. Best for masks (Game of Life) and intensity.
    grayscale(t) {
        const l = t * 255;
        return [l, l, l];
    },
    // Spectrum: t==0 stays black (e.g. the Mandelbrot set itself); everything
    // else sweeps two vivid hue cycles, revealing escape-time bands.
    spectrum(t) {
        if (t <= 0) return [0, 0, 0];
        return hsvToRgb((t * 720) % 360, 1, 1);
    },
    // Fire: black→red→orange→yellow→white heat ramp.
    fire(t) {
        const r = Math.min(1, t * 3);
        const g = Math.min(1, Math.max(0, t * 3 - 1));
        const b = Math.min(1, Math.max(0, t * 3 - 2));
        return [r * 255, g * 255, b * 255];
    },
};

// Normalize the buffer's values and paint into the canvas via `colormap`. The
// grid is canvas.width x canvas.height; the canvas may be displayed larger via
// CSS (use `image-rendering: pixelated`).
function paint(ctx, width, height, values, colormap) {
    const map = COLORMAPS[colormap] ?? COLORMAPS.grayscale;
    const total = width * height;
    let min = Infinity;
    let max = -Infinity;
    for (let i = 0; i < Math.min(total, values.length); i++) {
        const v = Number(values[i]);
        if (Number.isFinite(v)) {
            if (v < min) min = v;
            if (v > max) max = v;
        }
    }
    const range = max > min ? max - min : 1;
    const image = ctx.createImageData(width, height);
    for (let i = 0; i < total; i++) {
        const raw = i < values.length ? Number(values[i]) : 0;
        const norm = Number.isFinite(raw) ? (raw - min) / range : 0;
        const [r, g, b] = map(norm);
        const off = i * 4;
        image.data[off] = Math.max(0, Math.min(255, Math.round(r)));
        image.data[off + 1] = Math.max(0, Math.min(255, Math.round(g)));
        image.data[off + 2] = Math.max(0, Math.min(255, Math.round(b)));
        image.data[off + 3] = 255;
    }
    ctx.putImageData(image, 0, 0);
}

/// Mount a Miri GPU demo described by `manifest` onto `canvas`.
/// Returns `{ stop() }`. Static demos paint one frame; demos with `framePasses`
/// animate via requestAnimationFrame, dispatching all passes per frame with ping-pong.
export async function mount(canvas, manifest, opts = {}) {
    if (!canvas) throw new MiriGpuError("mount: a canvas element is required");
    if (!manifest || !manifest.buffers) throw new MiriGpuError("mount: invalid manifest");

    const width = manifest.canvas.width;
    const height = manifest.canvas.height;
    const colormap = opts.colormap ?? "grayscale";
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d");

    const device = await initGpu(opts);
    const buffers = createBuffers(device, manifest);
    const bufferOf = (name) => {
        const entry = buffers.get(name);
        if (!entry) throw new MiriGpuError(`manifest references unknown buffer '${name}'`);
        return entry;
    };

    // Seed kernels: compile + dispatch once, binding by name.
    for (const kernel of manifest.seed ?? []) {
        const compiled = compilePipeline(device, kernel);
        dispatchKernel(device, compiled, kernel, (name) => bufferOf(name).buffer);
    }

    const paintBuffer = bufferOf(manifest.paint);

    // Determine whether this is static (no animation) or animated.
    // New multi-pass syntax: framePasses is an array. Old single-pass: frame is a Kernel.
    const framePasses = manifest.framePasses ?? (manifest.frame ? [manifest.frame] : null);

    // Static demo: run-once already done by seed; paint a single frame.
    if (!framePasses) {
        await device.queue.onSubmittedWorkDone();
        const view = await readBackInto(
            device,
            paintBuffer.buffer,
            paintBuffer.length * typedArrayFor(paintBuffer.elemType).BYTES_PER_ELEMENT,
            typedArrayFor(paintBuffer.elemType),
        );
        paint(ctx, width, height, view, colormap);
        return { stop() {} };
    }

    // Animated demo: dispatch all frame passes in order each animation frame.
    // For passes with ping-pong buffers (first/last with read/write):
    // swap read/write between frames to alternate direction.
    const firstPass = framePasses[0];
    const lastPass = framePasses[framePasses.length - 1];
    const compiledPasses = framePasses.map((pass) => compilePipeline(device, pass));

    // Ping-pong buffers are only used in multi-pass; extract from first and last passes.
    let read = firstPass.read ? bufferOf(firstPass.read).buffer : null;
    let write = lastPass.write ? bufferOf(lastPass.write).buffer : null;

    const ArrayType = typedArrayFor(paintBuffer.elemType);
    const paintBytes = paintBuffer.length * ArrayType.BYTES_PER_ELEMENT;

    let running = true;
    let rafId = null;

    const step = async () => {
        if (!running) return;
        // Dispatch all passes in order.
        for (let i = 0; i < framePasses.length; i++) {
            const pass = framePasses[i];
            const compiled = compiledPasses[i];
            // For multi-pass: first pass uses 'read' ping-pong, last uses 'write'.
            // Middle passes bind their own buffers.
            let bindBuffer = (name) => {
                // First pass: its read → current read, its write → intermediate buffer
                if (i === 0 && name === pass.read && read) return read;
                // Last pass: its write → current write
                if (i === framePasses.length - 1 && name === pass.write && write) return write;
                // All passes: other names → their own buffers
                return bufferOf(name).buffer;
            };
            dispatchKernel(device, compiled, pass, bindBuffer);
        }
        await device.queue.onSubmittedWorkDone();
        // Paint from the last pass's write buffer (or the paint buffer if no write).
        const paintSource = write || paintBuffer.buffer;
        const view = await readBackInto(device, paintSource, paintBytes, ArrayType);
        if (!running) return;
        paint(ctx, width, height, view, colormap);
        // Swap: next frame reads what we just wrote.
        if (read && write) {
            const tmp = read;
            read = write;
            write = tmp;
        }
        rafId = requestAnimationFrame(step);
    };

    rafId = requestAnimationFrame(step);

    return {
        stop() {
            running = false;
            if (rafId !== null) cancelAnimationFrame(rafId);
        },
    };
}
