// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=1c9f8fcdeec4c37a96490125c8230d453673b495dad4fe7c53546a701aafaf05

// binary-trees: allocate and walk many complete binary trees (see binary-trees.mi).

class TreeNode {
  constructor(
    readonly left: TreeNode | null,
    readonly right: TreeNode | null,
  ) {}
}

function bottomUp(depth: number): TreeNode {
  if (depth === 0) {
    return new TreeNode(null, null);
  }
  return new TreeNode(bottomUp(depth - 1), bottomUp(depth - 1));
}

function check(node: TreeNode): number {
  if (node.left === null || node.right === null) {
    return 1;
  }
  return 1 + check(node.left) + check(node.right);
}

function kernel(maxDepth: number): string {
  const stretch = check(bottomUp(maxDepth + 1));
  const longLived = bottomUp(maxDepth);
  let total = 0;
  for (let depth = 4; depth <= maxDepth; depth += 2) {
    const iterations = 2 ** (maxDepth - depth + 4);
    for (let i = 0; i < iterations; i++) {
      total += check(bottomUp(depth));
    }
  }
  return `${stretch} ${total} ${check(longLived)}`;
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
