// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Miri WebGPU runtime shim.
//
// Standalone ES module driven by the HTML harness emitted by
// `miri build --target web-gpu`. NOT linked into the Miri binary.
//
// Surface:
//   initGpu(options?)                       → { adapter, device }
//   createStorageBuffer(device, source, o?) → GPUBuffer (storage + copy_src/dst)
//   createReadbackBuffer(device, byteLength)→ GPUBuffer (map_read + copy_dst)
//   compileShader(device, wgsl, label?)     → GPUShaderModule
//   dispatch(spec)                          → Promise<void>
//   readBuffer(device, buf, byteLength, T)  → Promise<TypedArray>
//   runKernel(spec)                         → Promise<TypedArray[]> (convenience)
//
// `spec` shapes match the layout the WGSL emitter produces:
//   @group(0) @binding(N) var<storage, read_write> <name>: array<T>;
//   @compute @workgroup_size(WX, WY, WZ)
//   fn <entry>(@builtin(global_invocation_id) ..., ...)

function storageUsage() {
    return GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST;
}

function readbackUsage() {
    return GPUBufferUsage.MAP_READ | GPUBufferUsage.COPY_DST;
}

export class MiriGpuError extends Error {
    constructor(message, cause) {
        super(message);
        this.name = "MiriGpuError";
        if (cause !== undefined) this.cause = cause;
    }
}

export async function initGpu(options = {}) {
    if (typeof navigator === "undefined" || !navigator.gpu) {
        throw new MiriGpuError(
            "WebGPU unavailable: navigator.gpu is undefined. " +
                "Use a WebGPU-capable browser (Chrome 113+, Edge 113+, Safari 18+ TP).",
        );
    }
    const adapter = await navigator.gpu.requestAdapter({
        powerPreference: options.powerPreference ?? "high-performance",
    });
    if (!adapter) {
        throw new MiriGpuError("requestAdapter() returned null — no GPU available");
    }
    const device = await adapter.requestDevice({
        requiredFeatures: options.requiredFeatures ?? [],
        requiredLimits: options.requiredLimits ?? {},
        label: options.deviceLabel ?? "miri-gpu-device",
    });
    device.lost.then((info) => {
        const reason = info?.reason ?? "unknown";
        console.error(`[miri-gpu] device lost (${reason}): ${info?.message ?? ""}`);
    });
    return { adapter, device };
}

function asTypedArray(source) {
    if (ArrayBuffer.isView(source)) return source;
    if (Array.isArray(source)) {
        const allInt = source.every((v) => Number.isInteger(v));
        return allInt ? new Int32Array(source) : new Float32Array(source);
    }
    throw new MiriGpuError(
        "createStorageBuffer: source must be a TypedArray or a plain Array of numbers",
    );
}

export function createStorageBuffer(device, source, options = {}) {
    const data = asTypedArray(source);
    const byteLength = alignTo(data.byteLength, 4);
    const buffer = device.createBuffer({
        label: options.label ?? "miri-storage",
        size: byteLength,
        usage: options.usage ?? storageUsage(),
        mappedAtCreation: true,
    });
    const mapped = buffer.getMappedRange();
    new Uint8Array(mapped).set(new Uint8Array(data.buffer, data.byteOffset, data.byteLength));
    buffer.unmap();
    return buffer;
}

export function createReadbackBuffer(device, byteLength, label = "miri-readback") {
    return device.createBuffer({
        label,
        size: alignTo(byteLength, 4),
        usage: readbackUsage(),
    });
}

export function compileShader(device, wgsl, label = "miri-kernel") {
    if (typeof wgsl !== "string" || wgsl.length === 0) {
        throw new MiriGpuError("compileShader: wgsl source must be a non-empty string");
    }
    return device.createShaderModule({ label, code: wgsl });
}

async function reportShaderDiagnostics(module, wgsl) {
    if (typeof module.getCompilationInfo !== "function") return;
    const info = await module.getCompilationInfo();
    const errors = info.messages.filter((m) => m.type === "error");
    if (errors.length === 0) return;
    const formatted = errors
        .map((m) => `  ${m.lineNum}:${m.linePos}: ${m.message}`)
        .join("\n");
    throw new MiriGpuError(
        `WGSL compilation failed:\n${formatted}\n--- source ---\n${wgsl}`,
    );
}

function buildBindGroupLayout(device, bindings) {
    const entries = bindings.map((b, i) => ({
        binding: b.binding ?? i,
        visibility: GPUShaderStage.COMPUTE,
        buffer: { type: b.access === "read" ? "read-only-storage" : "storage" },
    }));
    return device.createBindGroupLayout({ entries, label: "miri-bgl" });
}

function buildBindGroup(device, layout, bindings) {
    const entries = bindings.map((b, i) => ({
        binding: b.binding ?? i,
        resource: { buffer: b.buffer },
    }));
    return device.createBindGroup({ layout, entries, label: "miri-bg" });
}

function normalizeWorkgroups(workgroups) {
    if (typeof workgroups === "number") return [workgroups, 1, 1];
    if (!Array.isArray(workgroups)) {
        throw new MiriGpuError("dispatch: workgroups must be a number or [x,y,z]");
    }
    const [x = 1, y = 1, z = 1] = workgroups;
    return [x, y, z];
}

export async function dispatch(spec) {
    const { device, wgsl, module: precompiled, entryPoint, bindings, workgroups, label } = spec;
    if (!device) throw new MiriGpuError("dispatch: missing device");
    if (!entryPoint) throw new MiriGpuError("dispatch: missing entryPoint");
    if (!Array.isArray(bindings)) throw new MiriGpuError("dispatch: bindings must be an array");

    const shaderSource = wgsl;
    const module = precompiled ?? compileShader(device, shaderSource, label ?? entryPoint);
    if (shaderSource !== undefined) await reportShaderDiagnostics(module, shaderSource);

    const bindGroupLayout = buildBindGroupLayout(device, bindings);
    const pipelineLayout = device.createPipelineLayout({
        bindGroupLayouts: [bindGroupLayout],
        label: "miri-pl",
    });
    const pipeline = await device.createComputePipelineAsync({
        layout: pipelineLayout,
        compute: { module, entryPoint },
        label: label ?? `${entryPoint}-pipeline`,
    });
    const bindGroup = buildBindGroup(device, bindGroupLayout, bindings);

    const encoder = device.createCommandEncoder({ label: "miri-encoder" });
    const pass = encoder.beginComputePass({ label: "miri-pass" });
    pass.setPipeline(pipeline);
    pass.setBindGroup(0, bindGroup);
    const [gx, gy, gz] = normalizeWorkgroups(workgroups);
    pass.dispatchWorkgroups(gx, gy, gz);
    pass.end();
    device.queue.submit([encoder.finish()]);
    await device.queue.onSubmittedWorkDone();
}

export async function readBuffer(device, srcBuffer, byteLength, TypedArrayCtor = Float32Array) {
    const size = alignTo(byteLength, 4);
    const readback = createReadbackBuffer(device, size);
    const encoder = device.createCommandEncoder({ label: "miri-readback-encoder" });
    encoder.copyBufferToBuffer(srcBuffer, 0, readback, 0, size);
    device.queue.submit([encoder.finish()]);
    await readback.mapAsync(GPUMapMode.READ, 0, size);
    const range = readback.getMappedRange(0, size);
    const elementSize = TypedArrayCtor.BYTES_PER_ELEMENT;
    if (byteLength % elementSize !== 0) {
        readback.unmap();
        readback.destroy();
        throw new MiriGpuError(
            `readBuffer: byteLength ${byteLength} not a multiple of ${TypedArrayCtor.name}.BYTES_PER_ELEMENT (${elementSize})`,
        );
    }
    const view = new TypedArrayCtor(range.slice(0, byteLength));
    readback.unmap();
    readback.destroy();
    return view;
}

export async function runKernel(spec) {
    const {
        device,
        wgsl,
        entryPoint,
        inputs = [],
        outputs = [],
        workgroups,
        label,
    } = spec;
    if (!device) throw new MiriGpuError("runKernel: missing device");

    const inputBuffers = inputs.map((arr, i) =>
        createStorageBuffer(device, arr, { label: `${entryPoint}-in-${i}` }),
    );
    const outputBuffers = outputs.map((out, i) => {
        if (out.buffer) return out.buffer;
        if (out.initialData) {
            return createStorageBuffer(device, out.initialData, {
                label: `${entryPoint}-out-${i}`,
            });
        }
        const byteLength = out.length * out.type.BYTES_PER_ELEMENT;
        return device.createBuffer({
            label: `${entryPoint}-out-${i}`,
            size: alignTo(byteLength, 4),
            usage: storageUsage(),
        });
    });

    const orderedBuffers = [...inputBuffers, ...outputBuffers];
    const bindings = orderedBuffers.map((buffer, i) => ({ binding: i, buffer }));

    try {
        await dispatch({ device, wgsl, entryPoint, bindings, workgroups, label });
        const results = [];
        for (let i = 0; i < outputs.length; i++) {
            const out = outputs[i];
            const byteLength = out.length * out.type.BYTES_PER_ELEMENT;
            results.push(await readBuffer(device, outputBuffers[i], byteLength, out.type));
        }
        return results;
    } finally {
        for (const buf of inputBuffers) buf.destroy();
        for (let i = 0; i < outputBuffers.length; i++) {
            if (!outputs[i].buffer) outputBuffers[i].destroy();
        }
    }
}

function alignTo(value, alignment) {
    const remainder = value % alignment;
    return remainder === 0 ? value : value + (alignment - remainder);
}

export const __private = { asTypedArray, alignTo, normalizeWorkgroups };
