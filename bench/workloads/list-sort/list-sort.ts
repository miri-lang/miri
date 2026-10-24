// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-25 by review-agent sha256=fb21e8099938b3ac0c25d3771744f38a893b0be93f7ce4cadea49628a39e5f55

// list-sort: sort generated numbers, then records by key (see list-sort.mi).

interface Rec {
  key: number;
  id: number;
}

function kernel(n: number): string {
  const numbers: number[] = [];
  const records: Rec[] = [];
  let state = 7;
  for (let i = 0; i < n; i++) {
    state = (state * 48271) % 2147483647;
    numbers.push(state % 1000000);
    records.push({ key: state % 1000, id: i });
  }
  numbers.sort((a, b) => a - b);
  const byKey = records.slice().sort((a, b) => (a.key !== b.key ? a.key - b.key : b.id - a.id));
  let numberHash = 0;
  let recordHash = 0;
  for (let i = 0; i < n; i++) {
    numberHash = (numberHash * 31 + numbers[i]) % 1000000007;
    recordHash = (recordHash * 31 + byKey[i].id) % 1000000007;
  }
  return `${numberHash} ${recordHash}`;
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
