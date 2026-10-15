// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=1ea51954acfa2b2aba5b3a9603ce4a07265ac4a65522d60ed9038b7cb57c2206

// sieve: the sieve of Eratosthenes (see sieve.mi).

function kernel(n: number): string {
  const composite: boolean[] = new Array(n + 1).fill(false);
  let count = 0;
  let sum = 0;
  for (let p = 2; p <= n; p++) {
    if (!composite[p]) {
      count += 1;
      sum = (sum + p) % 1000000007;
      for (let m = p * p; m <= n; m += p) {
        composite[m] = true;
      }
    }
  }
  return `${count} ${sum}`;
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
