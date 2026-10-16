// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=23cd1e31104d704c901b5698c51a71d0edde5181f54061505e281d96c2ac4540

// enum-vm: a twelve-instruction register machine (see enum-vm.mi).
//
// JavaScript's `&` works on 32-bit integers; every operand the program masks
// stays below 2^31 for any loop count this workload runs, so it is exact.

type Instr =
  | { op: "const"; d: number; v: number }
  | { op: "mov"; d: number; a: number }
  | { op: "add" | "sub" | "mul" | "mod" | "and"; d: number; a: number; b: number }
  | { op: "addImm"; d: number; v: number }
  | { op: "jumpIfLess" | "jumpIfEqual"; a: number; b: number; target: number }
  | { op: "jump"; target: number }
  | { op: "halt" };

function program(n: number): Instr[] {
  const k = (d: number, v: number): Instr => ({ op: "const", d, v });
  const r = (op: "add" | "sub" | "mul" | "mod" | "and", d: number, a: number, b: number): Instr => ({ op, d, a, b });
  return [
    k(0, 0), k(1, 0), k(2, n), k(5, 97), k(6, 63), k(4, 1000000007), k(8, 0), k(9, 3),
    { op: "jumpIfLess", a: 0, b: 2, target: 10 }, { op: "halt" }, r("mul", 3, 0, 0), r("mod", 3, 3, 5),
    r("and", 3, 3, 6), r("add", 1, 1, 3), r("and", 7, 0, 6), { op: "jumpIfEqual", a: 7, b: 8, target: 18 },
    r("sub", 10, 2, 0), { op: "jump", target: 19 }, { op: "mov", d: 10, a: 9 }, r("mod", 10, 10, 9),
    r("add", 1, 1, 10), r("mod", 1, 1, 4), { op: "addImm", d: 0, v: 1 }, { op: "jump", target: 8 },
  ];
}

function kernel(n: number): string {
  const code = program(n);
  const regs: number[] = new Array(16).fill(0);
  let pc = 0;
  let steps = 0;
  let running = true;
  while (running) {
    const i = code[pc];
    let next = pc + 1;
    switch (i.op) {
      case "const":
        regs[i.d] = i.v;
        break;
      case "mov":
        regs[i.d] = regs[i.a];
        break;
      case "add":
        regs[i.d] = regs[i.a] + regs[i.b];
        break;
      case "sub":
        regs[i.d] = regs[i.a] - regs[i.b];
        break;
      case "mul":
        regs[i.d] = regs[i.a] * regs[i.b];
        break;
      case "mod":
        regs[i.d] = regs[i.a] % regs[i.b];
        break;
      case "and":
        regs[i.d] = regs[i.a] & regs[i.b];
        break;
      case "addImm":
        regs[i.d] = regs[i.d] + i.v;
        break;
      case "jumpIfLess":
        next = regs[i.a] < regs[i.b] ? i.target : next;
        break;
      case "jumpIfEqual":
        next = regs[i.a] === regs[i.b] ? i.target : next;
        break;
      case "jump":
        next = i.target;
        break;
      case "halt":
        running = false;
        break;
    }
    pc = next;
    steps += 1;
  }
  return `${regs[1]} ${steps}`;
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
