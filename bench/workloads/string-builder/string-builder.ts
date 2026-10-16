// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=9d2b5ddcb0749bfd7d439b74dbb3341dfb576be175bc1e0eb67f01d7b648526f

// string-builder: build a long string by appending (see string-builder.mi).

function kernel(n: number): string {
  let text = "";
  for (let i = 0; i < n; i++) {
    text += `${i}:${(i * i) % 1000};`;
  }
  let hash = 0;
  for (let k = 0; k < text.length; k++) {
    hash = (hash * 31 + text.charCodeAt(k)) % 1000000007;
  }
  return `${text.length} ${hash}`;
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
