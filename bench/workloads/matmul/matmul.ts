// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=d1af358d912567b0494d381b67acbe7a838aa97d46c37586c38ee4ad3ca0d7d1

// matmul: the naive triple-loop matrix product (see matmul.mi).

function matrix(n: number, offset: number): number[][] {
  return Array.from({ length: n }, (_, i) => Array.from({ length: n }, (_, j) => (i * j + offset * (i + j)) % 10));
}

function kernel(n: number): string {
  const a = matrix(n, 1);
  const b = matrix(n, 2);
  let total = 0.0;
  for (const row of a) {
    for (let j = 0; j < n; j++) {
      let sum = 0.0;
      for (let k = 0; k < n; k++) {
        sum = sum + row[k] * b[k][j];
      }
      total = total + sum;
    }
  }
  return `${Math.trunc(total)}`;
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
