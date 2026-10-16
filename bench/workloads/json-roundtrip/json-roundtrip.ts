// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=e71023e932f59e00ae80e5f443a0161f92c0f44914572031e89e95db8fa0af76

// json-roundtrip: parse, walk and rewrite a JSON document (see json-roundtrip.mi).

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

interface Totals {
  nodes: number;
  sum: number;
}

function walk(value: Json, totals: Totals): void {
  totals.nodes += 1;
  if (Array.isArray(value)) {
    for (const item of value) {
      walk(item, totals);
    }
  } else if (value !== null && typeof value === "object") {
    for (const key in value) {
      if (Object.hasOwn(value, key)) {
        walk(value[key], totals);
      }
    }
  } else if (typeof value === "number") {
    totals.sum += value;
  }
}

function kernel(text: string): string {
  const document: Json = JSON.parse(text);
  const totals: Totals = { nodes: 0, sum: 0 };
  walk(document, totals);
  const written = JSON.stringify(document);
  return `${totals.nodes} ${totals.sum} ${written.length}`;
}

function nowNs(): bigint {
  return BigInt(Math.round(performance.now() * 1e6));
}

function runHot(input: string): string {
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

const input = Deno.readTextFileSync(Deno.args[0]);
console.log(Deno.args[1] === "hot" ? runHot(input) : kernel(input));
