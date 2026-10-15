// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=8f5325264d9729e29a5f660e9dbeb8935eaa83c078d8a155ac4c57a5627c140f

// mandelbrot: the Mandelbrot set packed as a bitmap (see mandelbrot.mi).

function staysBounded(cr: number, ci: number): boolean {
  let zr = 0.0, zi = 0.0;
  for (let k = 0; k < 50; k++) {
    const tr = zr * zr - zi * zi + cr;
    zi = 2.0 * zr * zi + ci;
    zr = tr;
    if (zr * zr + zi * zi > 4.0) {
      return false;
    }
  }
  return true;
}

function kernel(n: number): string {
  let hash = 0;
  let inside = 0;
  for (let y = 0; y < n; y++) {
    const ci = (2.0 * y) / n - 1.0;
    let byte = 0, bits = 0;
    for (let x = 0; x < n; x++) {
      const cr = (2.0 * x) / n - 1.5;
      byte = byte * 2;
      if (staysBounded(cr, ci)) {
        byte += 1;
        inside += 1;
      }
      bits += 1;
      if (bits === 8) {
        hash = (hash * 31 + byte) % 1000000007;
        byte = 0;
        bits = 0;
      }
    }
    if (bits > 0) {
      byte = byte << (8 - bits);
      hash = (hash * 31 + byte) % 1000000007;
    }
  }
  return `${hash} ${inside}`;
}

function nowNs(): bigint {
  return BigInt(Math.round(performance.now() * 1e6));
}

function runHot(n: number): string {
  const expected = kernel(n);
  let agree = true;
  for (let i = 1; i < 5; i++) {
    const checksum = kernel(n);
    agree = agree && checksum === expected;
  }
  for (let i = 0; i < 5; i++) {
    const start = nowNs();
    const checksum = kernel(n);
    const elapsed = nowNs() - start;
    console.error(`hot_ns=${elapsed}`);
    agree = agree && checksum === expected;
  }
  if (!agree) {
    console.error("checksum differs between iterations");
    Deno.exit(1);
  }
  return expected;
}

const n = parseInt(Deno.readTextFileSync(Deno.args[0]).trim(), 10);
console.log(Deno.args[1] === "hot" ? runHot(n) : kernel(n));
