// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=1071dd8d648d88471f9a133f2ab6254e2617cbf67d5623ac7eb7c2929ac01c09

// generics: one generic stack at three element types (see generics.mi).

class Stack<T> {
  private items: T[] = [];

  push(item: T): void {
    this.items.push(item);
  }

  pop(): T | undefined {
    return this.items.pop();
  }

  size(): number {
    return this.items.length;
  }
}

interface Pair<A, B> {
  first: A;
  second: B;
}

function kernel(n: number): string {
  const ints = new Stack<number>();
  const floats = new Stack<number>();
  const pairs = new Stack<Pair<number, number>>();
  for (let i = 0; i < n; i++) {
    ints.push(i % 1000);
    floats.push((i % 100) * 0.5);
    pairs.push({ first: i % 10, second: i % 4 });
  }
  let a = 0;
  while (ints.size() > 0) {
    a = (a * 31 + (ints.pop() ?? 0)) % 1000000007;
  }
  let b = 0.0;
  while (floats.size() > 0) {
    b = b + (floats.pop() ?? 0);
  }
  let c = 0;
  while (pairs.size() > 0) {
    const p = pairs.pop()!;
    c = (c * 7 + p.first + Math.trunc(p.second)) % 1000000007;
  }
  return `${a} ${Math.trunc(b)} ${c}`;
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
