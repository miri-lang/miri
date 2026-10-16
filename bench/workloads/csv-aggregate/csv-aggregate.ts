// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=9df16ae8967eb62b4b2e228643125be58dd3e86b1c8e759e2e3aa1425b95f459

// csv-aggregate: totals by key over a generated CSV file (see csv-aggregate.mi).

interface Totals {
  count: number;
  quantity: number;
  amount: number;
}

function kernel(text: string): string {
  const groups = new Map<string, Totals>();
  for (const line of text.split("\n").slice(1)) {
    if (line === "") {
      continue;
    }
    const fields = line.split(",");
    let t = groups.get(fields[0]);
    if (t === undefined) {
      t = { count: 0, quantity: 0, amount: 0.0 };
      groups.set(fields[0], t);
    }
    t.count += 1;
    t.quantity += parseInt(fields[1], 10);
    t.amount = t.amount + parseFloat(fields[2]);
  }
  return [...groups.keys()]
    .sort()
    .map((key) => {
      const t = groups.get(key)!;
      return `${key}:${t.count}:${t.quantity}:${Math.trunc(t.amount * 100.0)}`;
    })
    .join(",");
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
