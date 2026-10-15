// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=93037a71c6b21c7ad7a8f23533341bd42dc58306f508176f473a769a8b8dca01

// spectral-norm: power iteration on an infinite matrix (see spectral-norm.mi).

function entry(i: number, j: number): number {
  return 1.0 / (((i + j) * (i + j + 1)) / 2 + i + 1);
}

function times(v: number[]): number[] {
  const n = v.length;
  const out: number[] = new Array(n);
  for (let i = 0; i < n; i++) {
    let sum = 0.0;
    for (let j = 0; j < n; j++) {
      sum = sum + entry(i, j) * v[j];
    }
    out[i] = sum;
  }
  return out;
}

function timesTransposed(v: number[]): number[] {
  const n = v.length;
  const out: number[] = new Array(n);
  for (let i = 0; i < n; i++) {
    let sum = 0.0;
    for (let j = 0; j < n; j++) {
      sum = sum + entry(j, i) * v[j];
    }
    out[i] = sum;
  }
  return out;
}

function timesBoth(v: number[]): number[] {
  return timesTransposed(times(v));
}

function kernel(n: number): string {
  let u: number[] = new Array(n).fill(1.0);
  let v: number[] = [];
  for (let round = 0; round < 10; round++) {
    v = timesBoth(u);
    u = timesBoth(v);
  }
  let vbv = 0.0, vv = 0.0;
  for (let i = 0; i < n; i++) {
    vbv = vbv + u[i] * v[i];
    vv = vv + v[i] * v[i];
  }
  return `${Math.trunc(Math.sqrt(vbv / vv) * 1000000000.0)}`;
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
