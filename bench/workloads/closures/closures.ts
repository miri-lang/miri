// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=30bf5fc020f73438657c68662498de8b30bb74fda36148842ca43b86b228d710

// closures: capturing map, filter and fold pipelines (see closures.mi).

function kernel(n: number): string {
  const xs = Array.from({ length: n }, (_, i) => i);
  let total = 0;
  for (let k = 1; k <= 8; k++) {
    const scale = k * 7;
    const threshold = 100 * k;
    const scaled = xs.map((x) => (x * scale) % 1000);
    const kept = scaled.filter((x) => x > threshold);
    total = (total + kept.reduce((a, b) => (a + b) % 1000000007, 0)) % 1000000007;
  }
  return `${total}`;
}

function nowNs(): bigint {
  return BigInt(Math.round(performance.now() * 1e6));
}

function runHot(input: number): string {
  const expected = kernel(input);
  let agree = true;
  for (let i = 1; i < 5; i++) {
    const checksum = kernel(input);
    agree = agree && checksum === expected;
  }
  for (let i = 0; i < 5; i++) {
    const start = nowNs();
    const checksum = kernel(input);
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

const input = parseInt(Deno.readTextFileSync(Deno.args[0]).trim(), 10);
console.log(Deno.args[1] === "hot" ? runHot(input) : kernel(input));
