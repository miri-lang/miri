// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=420a01bd395e43107057dfc426781f703c723d084a61a746b21d3b4e5ac8a5cd

// fannkuch-redux: pancake flips over every permutation (see fannkuch-redux.mi).

function countFlips(perm: number[], p: number[]): number {
  for (let i = 0; i < perm.length; i++) {
    p[i] = perm[i];
  }
  let flips = 0;
  let first = p[0];
  while (first !== 0) {
    for (let lo = 0, hi = first; lo < hi; lo++, hi--) {
      const t = p[lo];
      p[lo] = p[hi];
      p[hi] = t;
    }
    flips += 1;
    first = p[0];
  }
  return flips;
}

function kernel(n: number): string {
  const perm = Array.from({ length: n }, (_, i) => i);
  const count: number[] = new Array(n).fill(0);
  const scratch: number[] = new Array(n).fill(0);
  let checksum = 0;
  let maxFlips = 0;
  let index = 0;
  for (;;) {
    const flips = countFlips(perm, scratch);
    maxFlips = Math.max(maxFlips, flips);
    checksum += index % 2 === 0 ? flips : -flips;
    index += 1;
    let r = 1;
    for (; r < n; r++) {
      const first = perm[0];
      for (let k = 0; k < r; k++) {
        perm[k] = perm[k + 1];
      }
      perm[r] = first;
      count[r] += 1;
      if (count[r] <= r) {
        break;
      }
      count[r] = 0;
    }
    if (r >= n) {
      return `${checksum} ${maxFlips}`;
    }
  }
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
