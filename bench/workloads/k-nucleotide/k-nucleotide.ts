// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=88cf18e986b5913b9a8ed97902b6b069f0a44f79425eef4c7c600c751493c6ed

// k-nucleotide: count the substrings of a DNA sequence (see k-nucleotide.mi).

const MOTIF = "GGTATTTTAATTTATAGT";

function frequencies(sequence: string, k: number): Map<string, number> {
  const counts = new Map<string, number>();
  for (let i = 0; i + k <= sequence.length; i++) {
    const key = sequence.substring(i, i + k);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return counts;
}

function listing(sequence: string, k: number): string {
  const counts = frequencies(sequence, k);
  return [...counts.keys()].sort().map((key) => `${key}=${counts.get(key)}`).join(" ");
}

function kernel(text: string): string {
  const sequence = text.trim();
  const parts = [listing(sequence, 1), listing(sequence, 2)];
  for (const length of [3, 4, 6, 12, 18]) {
    const prefix = MOTIF.substring(0, length);
    parts.push(`${prefix}=${frequencies(sequence, length).get(prefix) ?? 0}`);
  }
  return parts.join(" ");
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
