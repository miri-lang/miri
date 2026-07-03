// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// Headless runner for a `miri build --target web-gpu` bundle.
//
// Boots the bundle's GPU compute path without a browser (no DOM, no canvas, no
// requestAnimationFrame) so CI can smoke-test that a bundle actually loads,
// uploads its buffers, dispatches its kernels, and reads back a result. Run it
// with any WebGPU-capable JS runtime:
//
//   deno run --allow-read --unstable-webgpu miri-gpu-headless.js <name>.json
//   node --experimental-webgpu miri-gpu-headless.js <name>.json   (WebGPU build)
//
// On success it prints `{ name, paint, values }` as JSON to stdout and exits 0.
// On any failure it prints the error to stderr and exits 1. When the runtime
// has no WebGPU, it exits 1 with a `MiriGpuError: WebGPU unavailable ...` — the
// bundle still parsed and imported, only the device request failed.
//
// `node:fs` and `process` are provided by both Node and Deno's node-compat
// layer, so the runner needs no runtime-specific branching.

import { readFileSync } from "node:fs";
import { runHeadless } from "./miri-gpu.js";

async function main() {
    const manifestPath = process.argv[2];
    if (!manifestPath) {
        process.stderr.write(
            "[miri-gpu-headless] usage: miri-gpu-headless.js <manifest.json> [frames]\n",
        );
        process.exit(2);
    }

    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    const frames = process.argv[3] !== undefined ? Number(process.argv[3]) : undefined;

    const result = await runHeadless(manifest, frames !== undefined ? { frames } : {});
    process.stdout.write(JSON.stringify(result) + "\n");
}

main().catch((err) => {
    process.stderr.write(`[miri-gpu-headless] ${err?.stack ?? err?.message ?? err}\n`);
    process.exit(1);
});
