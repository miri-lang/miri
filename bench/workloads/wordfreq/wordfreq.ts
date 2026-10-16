// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=b8a367d6bf0ae23ec09cba97d664f20fdf0ca27465e92f0f9cbb94addd5b851b

// wordfreq: count the words of a text (see wordfreq.mi).

function kernel(text: string): string {
  const counts = new Map<string, number>();
  for (const line of text.split("\n")) {
    for (const word of line.split(" ")) {
      if (word !== "") {
        counts.set(word, (counts.get(word) ?? 0) + 1);
      }
    }
  }
  const ranked = [...counts.entries()].sort((a, b) =>
    a[1] !== b[1] ? b[1] - a[1] : a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0
  );
  const top = ranked.slice(0, 10).map(([word, count]) => `${word}=${count}`).join(",");
  return `${counts.size} ${top}`;
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
