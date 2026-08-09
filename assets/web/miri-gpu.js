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
    // Raise the storage-buffer limits to what the adapter actually supports.
    // The default `maxStorageBuffersPerShaderStage` is only 8, but a single
    // kernel can bind more (e.g. the fluid demo's splat pass touches 10
    // ping-ponged fields); without this the pipeline silently fails to create
    // and those passes never run. Every value is copied straight from the
    // adapter, so none can exceed what the device allows.
    const l = adapter.limits;
    const requiredLimits = {
        maxStorageBuffersPerShaderStage: l.maxStorageBuffersPerShaderStage,
        maxStorageBufferBindingSize: l.maxStorageBufferBindingSize,
        maxBufferSize: l.maxBufferSize,
        maxBindGroups: l.maxBindGroups,
    };
    const device = await adapter.requestDevice({
        label: "miri-gpu-device",
        requiredLimits,
    });
    device.lost.then((info) => {
        if (info && info.reason !== "destroyed") {
            console.error(`[miri-gpu] device lost (${info.reason}): ${info.message ?? ""}`);
        }
    });
    return device;
}

// One GPUDevice is shared by every mount on a page. A page showing eight demos
// would otherwise request eight devices, each with its own driver-side state and
// memory budget, for work that can all be submitted through one. Acquisition is
// memoized on the in-flight promise, so mounts started concurrently still share.
let sharedDevice = null;

// Acquire the page's shared device, creating it on first use. A device that is
// lost (a driver reset, a background tab reclaimed) is forgotten here so the
// next mount transparently builds a fresh one instead of binding to a dead
// handle. `runHeadless` deliberately does not use this: it owns and destroys its
// device, which would take every other mount down with it.
function acquireDevice(opts) {
    if (sharedDevice) return sharedDevice;
    sharedDevice = initGpu(opts).then((device) => {
        if (device.lost && typeof device.lost.then === "function") {
            device.lost.then(() => {
                sharedDevice = null;
            });
        }
        return device;
    });
    sharedDevice.catch(() => {
        sharedDevice = null;
    });
    return sharedDevice;
}

const STORAGE_USAGE =
    (typeof GPUBufferUsage !== "undefined" &&
        GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST) ||
    0;

// Release every GPU allocation a mount made. The shared device is deliberately
// left alone — other mounts are still using it. Destroying is best-effort so a
// partially built mount can still be torn down after a failure.
function destroyAll(resources) {
    for (const resource of resources) {
        if (resource && typeof resource.destroy === "function") {
            try {
                resource.destroy();
            } catch {
                // A buffer already destroyed, or a stub without the API.
            }
        }
    }
    resources.length = 0;
}

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

const UNIFORM_USAGE =
    (typeof GPUBufferUsage !== "undefined" &&
        (GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST)) ||
    0;

function alignTo16(n) {
    return n % 16 === 0 ? n : n + (16 - (n % 16));
}

// The compiler lowers a frame pass's `frame.*` reads to a single uniform block,
// `@binding(N) var<uniform> _inputs: _Inputs`, whose binding index follows the
// pass's storage buffers. A render pass that never reads `frame.*` omits the
// block entirely. Parse the emitted WGSL — the ground truth — so the bind group
// layout matches the shader's declared bindings exactly, independent of how the
// storage buffers are counted.
function inputsBinding(kernel) {
    if (!kernel.inputs || kernel.inputs.length === 0) return null;
    const m = /@binding\((\d+)\)\s+var<uniform>\s+_inputs\b/.exec(kernel.wgsl);
    return m ? Number(m[1]) : null;
}

// Mutable per-frame input values written into the `_inputs` uniform each frame.
// Deltas (pointer drag, wheel, and the click pulses) accumulate between
// animation frames and are cleared after each dispatch; positional and held
// state (pointer position, `mouse_down`) persists across frames.
function newInputState() {
    return {
        time: 0,
        dt: 0,
        index: 0,
        mouse_x: 0,
        mouse_y: 0,
        mouse_down: 0,
        drag_dx: 0,
        drag_dy: 0,
        wheel: 0,
        clicked: 0,
        double_clicked: 0,
        move_x: 0,
        move_y: 0,
        hovering: 0,
    };
}

// Allocate the uniform buffer for a pass's `_inputs` block, or null when the
// pass declares no frame-input uniform. Size is the block's highest field end,
// rounded up to the 16-byte uniform-binding alignment WebGPU requires.
function createInputUniform(device, kernel) {
    const binding = inputsBinding(kernel);
    if (binding === null) return null;
    const maxEnd = kernel.inputs.reduce((m, f) => Math.max(m, f.offset + 4), 0);
    const byteLength = alignTo16(maxEnd);
    const buffer = device.createBuffer({
        label: `${kernel.entryPoint}-inputs`,
        size: byteLength,
        usage: UNIFORM_USAGE,
    });
    return { buffer, binding, byteLength, fields: kernel.inputs };
}

// Pack the current input state into the uniform buffer per the manifest's field
// layout (name → offset → wire type). Bool fields arrive as `u32`.
function writeInputUniform(device, uniform, state) {
    const data = new ArrayBuffer(uniform.byteLength);
    const view = new DataView(data);
    for (const f of uniform.fields) {
        const v = Number(state[f.name] ?? 0);
        if (f.ty === "f32") view.setFloat32(f.offset, v, true);
        else if (f.ty === "u32") view.setUint32(f.offset, v >>> 0, true);
        else view.setInt32(f.offset, v | 0, true);
    }
    device.queue.writeBuffer(uniform.buffer, 0, data);
}

function compilePipeline(device, kernel) {
    const module = device.createShaderModule({ label: kernel.entryPoint, code: kernel.wgsl });
    const entries = kernel.bindings.map((b, i) => ({
        binding: i,
        visibility: GPUShaderStage.COMPUTE,
        buffer: { type: b.access === "read" ? "read-only-storage" : "storage" },
    }));
    const uniformBinding = inputsBinding(kernel);
    if (uniformBinding !== null) {
        entries.push({
            binding: uniformBinding,
            visibility: GPUShaderStage.COMPUTE,
            buffer: { type: "uniform" },
        });
    }
    const layout = device.createBindGroupLayout({ label: `${kernel.entryPoint}-bgl`, entries });
    const pipeline = device.createComputePipeline({
        label: `${kernel.entryPoint}-pipeline`,
        layout: device.createPipelineLayout({ bindGroupLayouts: [layout] }),
        compute: { module, entryPoint: kernel.entryPoint },
    });
    return { pipeline, layout };
}

// Dispatch one kernel. `resolve(name)` maps a binding name to the GPUBuffer to
// bind there, letting the caller swap physical buffers for ping-pong. `uniform`
// (or null) is the pass's `_inputs` uniform, bound at its declared index.
function dispatchKernel(device, compiled, kernel, resolve, uniform) {
    const entries = kernel.bindings.map((b, i) => ({
        binding: i,
        resource: { buffer: resolve(b.name) },
    }));
    if (uniform) {
        entries.push({ binding: uniform.binding, resource: { buffer: uniform.buffer } });
    }
    const bindGroup = device.createBindGroup({
        label: `${kernel.entryPoint}-bg`,
        layout: compiled.layout,
        entries,
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

// GPU present: blit the compute output straight to the canvas with a fullscreen
// render pass — no CPU readback. Generic across paint modes: `rgba` reads four
// f32 channels per pixel; a colormap mode reads one scalar (f32/i32/u32,
// reinterpreted from the raw storage word) and maps it through a normalized
// palette. Normalization uses a per-frame GPU min/max (see REDUCE_WGSL) so the
// palette auto-fits the data, matching the former CPU colormap behavior.
const PRESENT_WGSL = `
struct Info { width: u32, height: u32, mode: u32, colormap: u32, elem: u32 };
@group(0) @binding(0) var<storage, read> paint: array<u32>;
@group(0) @binding(1) var<uniform> info: Info;
@group(0) @binding(2) var<storage, read> minmax: array<f32>;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    var pos = array<vec2<f32>, 3>(vec2<f32>(-1.0, -1.0), vec2<f32>(3.0, -1.0), vec2<f32>(-1.0, 3.0));
    return vec4<f32>(pos[vi], 0.0, 1.0);
}

fn scalar_at(i: u32) -> f32 {
    let w = paint[i];
    if (info.elem == 1u) { return f32(bitcast<i32>(w)); }
    if (info.elem == 2u) { return f32(w); }
    return bitcast<f32>(w);
}

fn hsv(h: f32, s: f32, v: f32) -> vec3<f32> {
    let c = v * s;
    let x = c * (1.0 - abs((h / 60.0) % 2.0 - 1.0));
    let m = v - c;
    var rgb = vec3<f32>(0.0, 0.0, 0.0);
    if (h < 60.0) { rgb = vec3<f32>(c, x, 0.0); }
    else if (h < 120.0) { rgb = vec3<f32>(x, c, 0.0); }
    else if (h < 180.0) { rgb = vec3<f32>(0.0, c, x); }
    else if (h < 240.0) { rgb = vec3<f32>(0.0, x, c); }
    else if (h < 300.0) { rgb = vec3<f32>(x, 0.0, c); }
    else { rgb = vec3<f32>(c, 0.0, x); }
    return rgb + vec3<f32>(m, m, m);
}

fn palette(t: f32) -> vec3<f32> {
    if (info.colormap == 1u) {
        if (t <= 0.0) { return vec3<f32>(0.0, 0.0, 0.0); }
        return hsv((t * 720.0) % 360.0, 1.0, 1.0);
    }
    if (info.colormap == 2u) {
        return vec3<f32>(clamp(t * 3.0, 0.0, 1.0), clamp(t * 3.0 - 1.0, 0.0, 1.0), clamp(t * 3.0 - 2.0, 0.0, 1.0));
    }
    return vec3<f32>(t, t, t);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = u32(frag.x);
    let py = u32(frag.y);
    if (px >= info.width || py >= info.height) { return vec4<f32>(0.0, 0.0, 0.0, 1.0); }
    let idx = py * info.width + px;
    if (info.mode == 1u) {
        let b = idx * 4u;
        return vec4<f32>(scalar_at(b), scalar_at(b + 1u), scalar_at(b + 2u), 1.0);
    }
    let v = scalar_at(idx);
    let mn = minmax[0];
    let mx = minmax[1];
    let range = select(mx - mn, 1.0, mx <= mn);
    let t = clamp((v - mn) / range, 0.0, 1.0);
    return vec4<f32>(palette(t), 1.0);
}
`;

// A single-workgroup strided min/max over the scalar paint buffer, feeding the
// colormap normalization. Reads the raw storage word and reinterprets it per
// the element type (matching PRESENT_WGSL's `scalar_at`).
const REDUCE_WGSL = `
struct RInfo { count: u32, elem: u32 };
@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> minmax: array<f32>;
@group(0) @binding(2) var<uniform> rinfo: RInfo;

var<workgroup> smin: array<f32, 256>;
var<workgroup> smax: array<f32, 256>;

fn value_at(i: u32) -> f32 {
    let w = src[i];
    if (rinfo.elem == 1u) { return f32(bitcast<i32>(w)); }
    if (rinfo.elem == 2u) { return f32(w); }
    return bitcast<f32>(w);
}

@compute @workgroup_size(256)
fn reduce_main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let t = lid.x;
    var mn = 3.0e38;
    var mx = -3.0e38;
    var i = t;
    loop {
        if (i >= rinfo.count) { break; }
        let v = value_at(i);
        mn = min(mn, v);
        mx = max(mx, v);
        i = i + 256u;
    }
    smin[t] = mn;
    smax[t] = mx;
    workgroupBarrier();
    var s = 128u;
    loop {
        if (s == 0u) { break; }
        if (t < s) {
            smin[t] = min(smin[t], smin[t + s]);
            smax[t] = max(smax[t], smax[t + s]);
        }
        workgroupBarrier();
        s = s / 2u;
    }
    if (t == 0u) {
        minmax[0] = smin[0];
        minmax[1] = smax[0];
    }
}
`;

const COLORMAP_ID = { grayscale: 0, spectrum: 1, fire: 2 };
const ELEM_ID = { f32: 0, i32: 1, u32: 2 };

// Configure a `webgpu` canvas context and build the fullscreen blit pipeline
// (plus, for colormap demos, the min/max reduction). Returns a presenter whose
// `presentFrame` draws a paint buffer to the canvas each frame with no readback.
function createPresenter(device, canvas, manifest, opts) {
    const ctx = canvas.getContext("webgpu");
    if (!ctx) throw new MiriGpuError("canvas.getContext('webgpu') returned null");
    const format = navigator.gpu.getPreferredCanvasFormat();
    ctx.configure({ device, format, alphaMode: "opaque" });

    const paintSpec = manifest.buffers.find((b) => b.name === manifest.paint);
    if (!paintSpec) {
        throw new MiriGpuError(`paint buffer '${manifest.paint}' is not declared in the manifest`);
    }
    const mode = manifest.paintMode === "rgba" ? 1 : 0;
    const elem = ELEM_ID[paintSpec.elemType] ?? 0;
    const colormap = COLORMAP_ID[opts.colormap] ?? 0;
    const width = manifest.canvas.width;
    const height = manifest.canvas.height;

    // Present uniform (Info): 5 x u32, padded to the 16-byte uniform alignment.
    const infoBuf = device.createBuffer({ label: "miri-present-info", size: 32, usage: UNIFORM_USAGE });
    device.queue.writeBuffer(infoBuf, 0, new Uint32Array([width, height, mode, colormap, elem]));

    // Shared min/max buffer: written by the reduction, read by the palette.
    const minmaxBuf = device.createBuffer({ label: "miri-present-minmax", size: 8, usage: STORAGE_USAGE });

    const presentModule = device.createShaderModule({ label: "miri-present", code: PRESENT_WGSL });
    const presentLayout = device.createBindGroupLayout({
        label: "miri-present-bgl",
        entries: [
            { binding: 0, visibility: GPUShaderStage.FRAGMENT, buffer: { type: "read-only-storage" } },
            { binding: 1, visibility: GPUShaderStage.FRAGMENT, buffer: { type: "uniform" } },
            { binding: 2, visibility: GPUShaderStage.FRAGMENT, buffer: { type: "read-only-storage" } },
        ],
    });
    const presentPipeline = device.createRenderPipeline({
        label: "miri-present-pipeline",
        layout: device.createPipelineLayout({ bindGroupLayouts: [presentLayout] }),
        vertex: { module: presentModule, entryPoint: "vs_main" },
        fragment: { module: presentModule, entryPoint: "fs_main", targets: [{ format }] },
        primitive: { topology: "triangle-list" },
    });

    // Colormap normalization needs a per-frame min/max; rgba passes final colors
    // through untouched and skips the reduction entirely.
    let reduce = null;
    if (mode === 0) {
        const rinfoBuf = device.createBuffer({ label: "miri-reduce-info", size: 16, usage: UNIFORM_USAGE });
        device.queue.writeBuffer(rinfoBuf, 0, new Uint32Array([paintSpec.length, elem]));
        const reduceModule = device.createShaderModule({ label: "miri-reduce", code: REDUCE_WGSL });
        const reduceLayout = device.createBindGroupLayout({
            label: "miri-reduce-bgl",
            entries: [
                { binding: 0, visibility: GPUShaderStage.COMPUTE, buffer: { type: "read-only-storage" } },
                { binding: 1, visibility: GPUShaderStage.COMPUTE, buffer: { type: "storage" } },
                { binding: 2, visibility: GPUShaderStage.COMPUTE, buffer: { type: "uniform" } },
            ],
        });
        const reducePipeline = device.createComputePipeline({
            label: "miri-reduce-pipeline",
            layout: device.createPipelineLayout({ bindGroupLayouts: [reduceLayout] }),
            compute: { module: reduceModule, entryPoint: "reduce_main" },
        });
        reduce = { pipeline: reducePipeline, layout: reduceLayout, rinfoBuf };
    }

    return { ctx, presentPipeline, presentLayout, infoBuf, minmaxBuf, reduce };
}

// Draw one frame: for a colormap demo, reduce the paint buffer's min/max, then
// blit it to the canvas. `paintBuffer` is the physical GPUBuffer holding this
// frame's output. All work rides one command buffer, so the passes execute in
// order (compute reduction before the render read).
function presentFrame(device, presenter, paintBuffer) {
    const encoder = device.createCommandEncoder();
    if (presenter.reduce) {
        const bindGroup = device.createBindGroup({
            layout: presenter.reduce.layout,
            entries: [
                { binding: 0, resource: { buffer: paintBuffer } },
                { binding: 1, resource: { buffer: presenter.minmaxBuf } },
                { binding: 2, resource: { buffer: presenter.reduce.rinfoBuf } },
            ],
        });
        const pass = encoder.beginComputePass();
        pass.setPipeline(presenter.reduce.pipeline);
        pass.setBindGroup(0, bindGroup);
        pass.dispatchWorkgroups(1);
        pass.end();
    }
    const bindGroup = device.createBindGroup({
        layout: presenter.presentLayout,
        entries: [
            { binding: 0, resource: { buffer: paintBuffer } },
            { binding: 1, resource: { buffer: presenter.infoBuf } },
            { binding: 2, resource: { buffer: presenter.minmaxBuf } },
        ],
    });
    const pass = encoder.beginRenderPass({
        colorAttachments: [
            {
                view: presenter.ctx.getCurrentTexture().createView(),
                clearValue: { r: 0, g: 0, b: 0, a: 1 },
                loadOp: "clear",
                storeOp: "store",
            },
        ],
    });
    pass.setPipeline(presenter.presentPipeline);
    pass.setBindGroup(0, bindGroup);
    pass.draw(3);
    pass.end();
    device.queue.submit([encoder.finish()]);
}

// Map a buffer name to its allocated device-buffer entry, erroring on an
// unknown name. Shared by the canvas `mount` and headless `runHeadless` paths.
function makeBufferResolver(buffers) {
    return (name) => {
        const entry = buffers.get(name);
        if (!entry) throw new MiriGpuError(`manifest references unknown buffer '${name}'`);
        return entry;
    };
}

// Seed kernels: compile + dispatch once, binding each kernel's buffers by name.
function runSeedKernels(device, manifest, bufferOf) {
    for (const kernel of manifest.seed ?? []) {
        const compiled = compilePipeline(device, kernel);
        dispatchKernel(device, compiled, kernel, (name) => bufferOf(name).buffer);
    }
}

// The animation's cross-frame ping-pong pairs. Buffers named `X_a`/`X_b` form a
// double-buffered pair; a pair swaps between frames when the side read at frame
// start (`source`) differs from the side written last (`dest`) — so the next
// frame reads exactly the value just produced. This covers the two common shapes
// at once:
//   • a one-directional step (weights `wa`→`wb`, camera `cam_a`→`cam_b`): read
//     side ≠ last-write side ⇒ swap.
//   • an in-frame round trip (fluid dye `dr_a`→`dr_b`→`dr_a`): read side ==
//     last-write side ⇒ no swap, the field already persists in place.
// A demo can ping-pong several fields independently (e.g. weights AND velocities)
// since every qualifying pair swaps. A name-paired buffer that is never written
// (a read-only input) is not a pair, so an input is never swapped with a scratch.
//
// Which side is read and which is written comes from each binding's `writes`
// flag, not from its WGSL `access` qualifier: atomic storage is always bound
// read-write, so a pair of atomic buffers would otherwise present as two
// destinations and no source, and never swap at all.
export function statePairs(framePasses) {
    const rec = new Map();
    const get = (base) => {
        let r = rec.get(base);
        if (!r) {
            r = { hasA: false, hasB: false, firstRead: null, lastWrite: null };
            rec.set(base, r);
        }
        return r;
    };
    for (const pass of framePasses) {
        for (const b of pass.bindings ?? []) {
            const m = /^(.*)_(a|b)$/.exec(b.name);
            if (m) (m[2] === "a" ? (get(m[1]).hasA = true) : (get(m[1]).hasB = true));
        }
    }
    for (const pass of framePasses) {
        for (const b of pass.bindings ?? []) {
            const m = /^(.*)_(a|b)$/.exec(b.name);
            if (!m) continue;
            const r = rec.get(m[1]);
            if (!r.hasA || !r.hasB) continue;
            // `writes` is the pass's data flow; `access` is only the WGSL
            // binding qualifier, and an atomic buffer is bound read-write even
            // where a pass only reads it. Keying on `access` therefore made a
            // pair of atomic buffers look like two destinations and no source,
            // which left the pair unregistered and never swapped.
            if (!b.writes && r.firstRead === null) r.firstRead = m[2];
            if (b.writes) r.lastWrite = m[2];
        }
    }
    const pairs = [];
    for (const [base, r] of rec) {
        if (!r.hasA || !r.hasB || r.firstRead === null || r.lastWrite === null) continue;
        if (r.firstRead !== r.lastWrite) {
            pairs.push({ source: `${base}_${r.firstRead}`, dest: `${base}_${r.lastWrite}` });
        }
    }
    return pairs;
}

// Track each swap pair's two physical buffers: `resolve` maps a binding name to
// its current-frame buffer (non-pair names resolve to themselves), and `swap`
// exchanges every pair's buffers after a frame so the next reads what this wrote.
function makeSwapState(pairs, bufferOf) {
    const phys = new Map();
    for (const p of pairs) {
        phys.set(p.source, bufferOf(p.source).buffer);
        phys.set(p.dest, bufferOf(p.dest).buffer);
    }
    return {
        resolve: (name) => phys.get(name) ?? bufferOf(name).buffer,
        swap: () => {
            for (const p of pairs) {
                const s = phys.get(p.source);
                phys.set(p.source, phys.get(p.dest));
                phys.set(p.dest, s);
            }
        },
    };
}

/// Run a Miri GPU demo headlessly (no canvas, no requestAnimationFrame) and
/// return the paint buffer's values. This is the CI smoke path a Node/Deno
/// runner drives to verify a `--target web-gpu` bundle actually boots and
/// dispatches without a browser. Static demos run their seed kernels once;
/// animated demos additionally run `opts.frames` (default 1) frame rounds with
/// ping-pong. Returns `{ name, paint, values }`.
export async function runHeadless(manifest, opts = {}) {
    if (!manifest || !manifest.buffers) throw new MiriGpuError("runHeadless: invalid manifest");

    const device = await initGpu(opts);
    try {
        const buffers = createBuffers(device, manifest);
        const bufferOf = makeBufferResolver(buffers);
        runSeedKernels(device, manifest, bufferOf);

        const paintBuffer = bufferOf(manifest.paint);
        const framePasses = manifest.framePasses ?? (manifest.frame ? [manifest.frame] : null);
        let outputBuffer = paintBuffer.buffer;

        if (framePasses) {
            const frames = Math.max(0, opts.frames ?? 1);
            const compiledPasses = framePasses.map((pass) => compilePipeline(device, pass));
            const passUniforms = framePasses.map((pass) => createInputUniform(device, pass));
            // Headless has no pointer/wheel source: feed zeroed inputs so passes
            // that read `frame.*` still validate and dispatch deterministically.
            const state = newInputState();
            const swapState = makeSwapState(statePairs(framePasses), bufferOf);
            const resolve = swapState.resolve;
            for (let f = 0; f < frames; f++) {
                for (let i = 0; i < framePasses.length; i++) {
                    if (passUniforms[i]) writeInputUniform(device, passUniforms[i], state);
                    dispatchKernel(device, compiledPasses[i], framePasses[i], resolve, passUniforms[i]);
                }
                // Read back whichever physical buffer holds this frame's paint
                // output before the pairs swap for the next frame.
                outputBuffer = resolve(manifest.paint);
                swapState.swap();
            }
        }

        await device.queue.onSubmittedWorkDone();
        const ArrayType = typedArrayFor(paintBuffer.elemType);
        const view = await readBackInto(
            device,
            outputBuffer,
            paintBuffer.length * ArrayType.BYTES_PER_ELEMENT,
            ArrayType,
        );
        return {
            name: manifest.name ?? null,
            paint: manifest.paint,
            values: Array.from(view, Number),
        };
    } finally {
        if (typeof device.destroy === "function") device.destroy();
    }
}

// Periodically read one buffer back so a page can show the numbers a demo
// computes — a training loss, a particle count — rather than only its picture.
//
// Presenting is deliberately readback-free: the paint buffer is blitted straight
// to the canvas, so frame rate is bound by the GPU rather than by copy latency.
// A numeric readout has no such path, since the values must reach JavaScript to
// be formatted. Sampling is therefore rate-limited (a few times a second, far
// below frame rate) and never overlaps itself, so a slow map cannot queue copies
// behind it. Returns null when the caller asked for no readout.
function createStatsSampler(device, manifest, resolve, opts) {
    if (typeof opts.onStats !== "function") return null;
    const name = opts.statsBuffer;
    const spec = manifest.buffers.find((b) => b.name === name);
    if (!spec) {
        throw new MiriGpuError(`mount: onStats needs a buffer named '${name}' in the manifest`);
    }
    const ArrayType = typedArrayFor(spec.elemType);
    const byteLength = spec.length * ArrayType.BYTES_PER_ELEMENT;
    const interval = 1 / Math.max(0.1, opts.statsHz ?? 4);
    // Start due, so the readout is populated on the first frame rather than
    // after the first interval.
    let since = interval;
    let inFlight = false;
    let stopped = false;
    let warned = false;

    return {
        tick(dt) {
            if (stopped || inFlight) return;
            since += dt;
            if (since < interval) return;
            since = 0;
            inFlight = true;
            readBackInto(device, resolve(name), byteLength, ArrayType)
                .then((view) => {
                    if (!stopped) opts.onStats(Array.from(view, Number));
                })
                .catch((err) => {
                    // A mount that stopped mid-copy rejects here as a matter of
                    // course; anything else is worth saying once.
                    if (stopped || warned) return;
                    warned = true;
                    console.warn(`[miri-gpu] stats readback failed: ${err?.message ?? err}`);
                })
                .finally(() => {
                    inFlight = false;
                });
        },
        stop() {
            stopped = true;
        },
    };
}

/// Mount a Miri GPU demo described by `manifest` onto `canvas`.
/// Returns `{ pause(), resume(), stop() }`. The paint buffer is drawn to the
/// canvas entirely on the GPU (a fullscreen render pass, no readback). Static
/// demos present one frame; demos with `framePasses` animate via
/// requestAnimationFrame, double-buffering the state pair each frame.
///
/// `opts.input: false` leaves the pointer unwired, for a demo shown as a preview
/// inside something the pointer is meant to reach — a card that is itself a
/// link, say. The demo still animates; it just does not steer.
///
/// `opts.onStats` with `opts.statsBuffer` reads that buffer back `opts.statsHz`
/// times a second (default 4) and hands its values to the callback, for a demo
/// that publishes numbers alongside its picture.
export async function mount(canvas, manifest, opts = {}) {
    if (!canvas) throw new MiriGpuError("mount: a canvas element is required");
    if (!manifest || !manifest.buffers) throw new MiriGpuError("mount: invalid manifest");

    canvas.width = manifest.canvas.width;
    canvas.height = manifest.canvas.height;

    const device = await acquireDevice(opts);
    const buffers = createBuffers(device, manifest);
    const bufferOf = makeBufferResolver(buffers);
    const presenter = createPresenter(device, canvas, manifest, opts);

    // Everything this mount allocated on the device, so `stop()` can give it
    // back. A page mounts demos as they scroll into view; without this, the
    // storage of every demo ever seen would stay resident for the session.
    const resources = [...buffers.values()].map((b) => b.buffer);
    resources.push(presenter.infoBuf, presenter.minmaxBuf);
    if (presenter.reduce) resources.push(presenter.reduce.rinfoBuf);

    // Seed kernels: compile + dispatch once, binding by name.
    runSeedKernels(device, manifest, bufferOf);

    // Determine whether this is static (no animation) or animated.
    // New multi-pass syntax: framePasses is an array. Old single-pass: frame is a Kernel.
    const framePasses = manifest.framePasses ?? (manifest.frame ? [manifest.frame] : null);

    // Static demo: run-once already done by seed; present a single frame.
    if (!framePasses) {
        presentFrame(device, presenter, bufferOf(manifest.paint).buffer);
        // A static demo has nothing to pause: its single frame is already on the
        // canvas and no work is scheduled.
        return {
            pause() {},
            resume() {},
            stop() {
                destroyAll(resources);
            },
        };
    }

    // Animated demo: dispatch all frame passes in order each animation frame,
    // double-buffering the state pair (see `statePair`) so each frame reads the
    // previous frame's result.
    const compiledPasses = framePasses.map((pass) => compilePipeline(device, pass));
    const passUniforms = framePasses.map((pass) => createInputUniform(device, pass));
    for (const uniform of passUniforms) {
        if (uniform) resources.push(uniform.buffer);
    }

    const swapState = makeSwapState(statePairs(framePasses), bufferOf);
    const resolve = swapState.resolve;
    const stats = createStatsSampler(device, manifest, resolve, opts);

    // Live pointer/wheel state feeding the `frame.*` uniforms. Only wired when a
    // pass actually reads inputs, so static and input-free demos add no listeners.
    const state = newInputState();
    const wantsInput = opts.input !== false && passUniforms.some((u) => u !== null);
    const detachInput = wantsInput ? attachInputListeners(canvas, state) : () => {};
    // The frame clock accumulates elapsed time rather than measuring against a
    // start timestamp, so a demo that was paused resumes with its own time
    // continuing instead of jumping forward by the wall time it spent away. A
    // pause sets `lastTime` to null, which makes the first frame back contribute
    // no elapsed time and no `dt` spike.
    let lastTime = null;
    let elapsed = 0;

    let running = true;
    let stopped = false;
    let rafId = null;

    const step = (now) => {
        if (!running) return;
        // `now` is the rAF timestamp in milliseconds.
        const t = typeof now === "number" ? now : 0;
        state.dt = lastTime === null ? 0 : (t - lastTime) / 1000;
        elapsed += state.dt;
        state.time = elapsed;
        lastTime = t;

        // Dispatch all passes in order against the shared resolver, so a later
        // pass reading a pair's `dest` sees this frame's write.
        for (let i = 0; i < framePasses.length; i++) {
            if (passUniforms[i]) writeInputUniform(device, passUniforms[i], state);
            dispatchKernel(device, compiledPasses[i], framePasses[i], resolve, passUniforms[i]);
        }
        // Per-frame deltas are consumed by this frame; clear them for the next.
        state.drag_dx = 0;
        state.drag_dy = 0;
        state.move_x = 0;
        state.move_y = 0;
        state.wheel = 0;
        state.clicked = 0;
        state.double_clicked = 0;

        // Present whichever physical buffer holds this frame's paint output (a
        // terminal target resolves to itself; a pair member to its live buffer).
        // GPU-resident: the compute output is blitted straight to the canvas with
        // no CPU readback, so animation is bound by the GPU, not by copy latency.
        presentFrame(device, presenter, resolve(manifest.paint));
        // Report the just-submitted frame so callers can derive an FPS readout.
        if (typeof opts.onFrame === "function") opts.onFrame(state.dt);
        // Sample before the swap, so the copy reads the buffer this frame wrote.
        if (stats) stats.tick(state.dt);
        // Swap every ping-pong pair: next frame reads what this frame produced.
        swapState.swap();
        rafId = requestAnimationFrame(step);
    };

    rafId = requestAnimationFrame(step);

    return {
        // Stop scheduling frames while keeping the demo's state on the device,
        // so an off-screen or backgrounded demo costs no GPU time and resumes
        // exactly where it left off. The frame clock is not advanced while
        // paused: `frame.time` continues rather than jumping by the time spent
        // away, which would make a simulation lurch on resume.
        pause() {
            if (!running) return;
            running = false;
            if (rafId !== null) cancelAnimationFrame(rafId);
            rafId = null;
        },
        resume() {
            if (running || stopped) return;
            running = true;
            // Re-anchor the clock so the gap counts as no elapsed time.
            lastTime = null;
            rafId = requestAnimationFrame(step);
        },
        stop() {
            if (stopped) return;
            stopped = true;
            running = false;
            if (stats) stats.stop();
            detachInput();
            if (rafId !== null) cancelAnimationFrame(rafId);
            rafId = null;
            destroyAll(resources);
        },
    };
}

// Wire pointer, wheel, and click events on `canvas` into `state`. Absolute
// pointer position (`mouse_x`/`mouse_y`) is reported in normalized [0,1] canvas
// space so a demo reads it independent of its render resolution; drag deltas
// (`drag_dx`/`drag_dy`) are reported in CSS pixels — the units the pointer
// itself moves in — so a demo's pan and orbit sensitivity is the same however
// large the canvas is drawn and whatever its backing-store resolution is.
// Deltas accumulate into `state` and are cleared by the caller each frame.
// Returns a detach function.
function attachInputListeners(canvas, state) {
    let dragging = false;
    let lastX = 0;
    let lastY = 0;

    const toNorm = (event) => {
        const rect = canvas.getBoundingClientRect();
        const nx = rect.width > 0 ? (event.clientX - rect.left) / rect.width : 0;
        const ny = rect.height > 0 ? (event.clientY - rect.top) / rect.height : 0;
        return [nx, ny];
    };

    // Previous normalized pointer position, for the per-frame `move_*` delta
    // (reported on hover, unlike the button-gated `drag_*`). null until the
    // first event so the initial jump from the origin registers no motion.
    let lastNX = null;
    let lastNY = null;

    const onEnter = (event) => {
        state.hovering = 1;
        [state.mouse_x, state.mouse_y] = toNorm(event);
        [lastNX, lastNY] = [state.mouse_x, state.mouse_y];
    };
    const onDown = (event) => {
        dragging = true;
        state.mouse_down = 1;
        state.hovering = 1;
        [lastX, lastY] = [event.clientX, event.clientY];
        [state.mouse_x, state.mouse_y] = toNorm(event);
        [lastNX, lastNY] = [state.mouse_x, state.mouse_y];
        // Capture the pointer so a drag that leaves the canvas keeps steering the
        // demo instead of stopping dead at the edge. Guarded because a headless
        // canvas stub has no pointer-capture API.
        if (typeof canvas.setPointerCapture === "function" && event.pointerId !== undefined) {
            canvas.setPointerCapture(event.pointerId);
        }
    };
    const onMove = (event) => {
        state.hovering = 1;
        [state.mouse_x, state.mouse_y] = toNorm(event);
        if (lastNX !== null) {
            state.move_x += state.mouse_x - lastNX;
            state.move_y += state.mouse_y - lastNY;
        }
        [lastNX, lastNY] = [state.mouse_x, state.mouse_y];
        if (dragging) {
            // Grab-and-pan: the image follows the pointer. The axes differ in
            // sign because the paint buffer's first row is the top of the canvas,
            // which flips the vertical (imaginary) axis relative to the pointer
            // but leaves the horizontal (real) axis aligned.
            state.drag_dx += event.clientX - lastX;
            state.drag_dy -= event.clientY - lastY;
            lastX = event.clientX;
            lastY = event.clientY;
        }
    };
    const onUp = (event) => {
        dragging = false;
        state.mouse_down = 0;
        if (
            typeof canvas.releasePointerCapture === "function" &&
            event &&
            event.pointerId !== undefined &&
            (typeof canvas.hasPointerCapture !== "function" ||
                canvas.hasPointerCapture(event.pointerId))
        ) {
            canvas.releasePointerCapture(event.pointerId);
        }
    };
    // Pointer left the canvas: end any drag, release the button, and clear the
    // hover flag so hover-driven demos settle. `move_*` deltas are left as-is
    // (they are cleared by the caller each frame).
    const onLeave = () => {
        dragging = false;
        state.mouse_down = 0;
        state.hovering = 0;
        lastNX = null;
        lastNY = null;
    };
    const onWheel = (event) => {
        event.preventDefault();
        // Pass the raw wheel delta through: a demo reading `frame.wheel` decides
        // the zoom direction (e.g. `exp(frame.wheel * k)`), so scroll-up
        // (negative deltaY) zooms in, matching the platform scroll convention.
        state.wheel += event.deltaY;
    };
    const onClick = () => {
        state.clicked = 1;
    };
    const onDblClick = () => {
        state.double_clicked = 1;
    };

    canvas.addEventListener("pointerenter", onEnter);
    canvas.addEventListener("pointerdown", onDown);
    canvas.addEventListener("pointermove", onMove);
    canvas.addEventListener("pointerup", onUp);
    canvas.addEventListener("pointerleave", onLeave);
    canvas.addEventListener("wheel", onWheel, { passive: false });
    canvas.addEventListener("click", onClick);
    canvas.addEventListener("dblclick", onDblClick);

    return () => {
        canvas.removeEventListener("pointerenter", onEnter);
        canvas.removeEventListener("pointerdown", onDown);
        canvas.removeEventListener("pointermove", onMove);
        canvas.removeEventListener("pointerup", onUp);
        canvas.removeEventListener("pointerleave", onLeave);
        canvas.removeEventListener("wheel", onWheel);
        canvas.removeEventListener("click", onClick);
        canvas.removeEventListener("dblclick", onDblClick);
    };
}
