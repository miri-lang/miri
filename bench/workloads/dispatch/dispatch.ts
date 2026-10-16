// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=b0feab5d7d6c87fba9180b9933b98df5bdec72fc5243d24153144de22e0f87e1

// dispatch: shapes behind one interface, their areas summed (see dispatch.mi).

interface Shape {
  area(): number;
}

class Circle implements Shape {
  constructor(readonly radius: number) {}
  area(): number {
    return 3.0 * this.radius * this.radius;
  }
}

class Square implements Shape {
  constructor(readonly side: number) {}
  area(): number {
    return this.side * this.side;
  }
}

class Rectangle implements Shape {
  constructor(readonly width: number, readonly height: number) {}
  area(): number {
    return this.width * this.height;
  }
}

class Triangle implements Shape {
  constructor(readonly base: number, readonly height: number) {}
  area(): number {
    return this.base * this.height * 0.5;
  }
}

function shape(i: number): Shape {
  const a = (i % 7) + 1;
  const b = (i % 5) + 1;
  switch (i % 4) {
    case 0:
      return new Circle(a);
    case 1:
      return new Square(a);
    case 2:
      return new Rectangle(a, b);
    default:
      return new Triangle(a, b);
  }
}

function kernel(n: number): string {
  const shapes: Shape[] = Array.from({ length: 1000 }, (_, i) => shape(i));
  let total = 0.0;
  for (let round = 0; round < n; round++) {
    for (const s of shapes) {
      total = total + s.area();
    }
  }
  return `${Math.trunc(total)}`;
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
