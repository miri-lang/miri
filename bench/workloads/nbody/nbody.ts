// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=2dd6d9b8ed94c1e7d00307475c1a51c04fdc76d68b5f548fc03ba9a7a28636d9

// nbody: the Jovian planets orbiting the sun (see nbody.mi).

const PI = 3.141592653589793;
const SOLAR_MASS = 4.0 * PI * PI;
const DAYS_PER_YEAR = 365.24;

interface Body {
  x: number;
  y: number;
  z: number;
  vx: number;
  vy: number;
  vz: number;
  mass: number;
}

function planet(x: number, y: number, z: number, vx: number, vy: number, vz: number, mass: number): Body {
  const d = DAYS_PER_YEAR;
  return { x, y, z, vx: vx * d, vy: vy * d, vz: vz * d, mass: mass * SOLAR_MASS };
}

function solarSystem(): Body[] {
  return [
    { x: 0.0, y: 0.0, z: 0.0, vx: 0.0, vy: 0.0, vz: 0.0, mass: SOLAR_MASS },
    planet(4.84143144246472090e+00, -1.16032004402742839e+00, -1.03622044471123109e-01, 1.66007664274403694e-03, 7.69901118419740425e-03, -6.90460016972063023e-05, 9.54791938424326609e-04),
    planet(8.34336671824457987e+00, 4.12479856412430479e+00, -4.03523417114321381e-01, -2.76742510726862411e-03, 4.99852801234917238e-03, 2.30417297573763929e-05, 2.85885980666130812e-04),
    planet(1.28943695621391310e+01, -1.51111514016986312e+01, -2.23307578892655734e-01, 2.96460137564761618e-03, 2.37847173959480950e-03, -2.96589568540237556e-05, 4.36624404335156298e-05),
    planet(1.53796971148509165e+01, -2.59193146099879641e+01, 1.79258772950371181e-01, 2.68067772490389322e-03, 1.62824170038242295e-03, -9.51592254519715870e-05, 5.15138902046611451e-05),
  ];
}

function offsetMomentum(bodies: Body[]): void {
  let px = 0.0, py = 0.0, pz = 0.0;
  for (const b of bodies) {
    px = px + b.vx * b.mass;
    py = py + b.vy * b.mass;
    pz = pz + b.vz * b.mass;
  }
  bodies[0].vx = -px / SOLAR_MASS;
  bodies[0].vy = -py / SOLAR_MASS;
  bodies[0].vz = -pz / SOLAR_MASS;
}

function energy(bodies: Body[]): number {
  let e = 0.0;
  for (let i = 0; i < bodies.length; i++) {
    const b = bodies[i];
    e = e + 0.5 * b.mass * (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz);
    for (let j = i + 1; j < bodies.length; j++) {
      const o = bodies[j];
      const dx = b.x - o.x;
      const dy = b.y - o.y;
      const dz = b.z - o.z;
      e = e - b.mass * o.mass / Math.sqrt(dx * dx + dy * dy + dz * dz);
    }
  }
  return e;
}

function advance(bodies: Body[], dt: number): void {
  for (let i = 0; i < bodies.length; i++) {
    const bi = bodies[i];
    for (let j = i + 1; j < bodies.length; j++) {
      const bj = bodies[j];
      const dx = bi.x - bj.x;
      const dy = bi.y - bj.y;
      const dz = bi.z - bj.z;
      const d2 = dx * dx + dy * dy + dz * dz;
      const mag = dt / (d2 * Math.sqrt(d2));
      const mi = bi.mass * mag;
      const mj = bj.mass * mag;
      bi.vx = bi.vx - dx * mj;
      bi.vy = bi.vy - dy * mj;
      bi.vz = bi.vz - dz * mj;
      bj.vx = bj.vx + dx * mi;
      bj.vy = bj.vy + dy * mi;
      bj.vz = bj.vz + dz * mi;
    }
  }
  for (const b of bodies) {
    b.x = b.x + dt * b.vx;
    b.y = b.y + dt * b.vy;
    b.z = b.z + dt * b.vz;
  }
}

function kernel(steps: number): string {
  const bodies = solarSystem();
  offsetMomentum(bodies);
  const before = energy(bodies);
  for (let s = 0; s < steps; s++) {
    advance(bodies, 0.01);
  }
  const after = energy(bodies);
  return `${Math.trunc(before * 1000000000.0)} ${Math.trunc(after * 1000000000.0)}`;
}

function nowNs(): bigint {
  return BigInt(Math.round(performance.now() * 1e6));
}

function runHot(n: number): string {
  const expected = kernel(n);
  let agree = true;
  for (let i = 1; i < 5; i++) {
    const checksum = kernel(n);
    agree = agree && checksum === expected;
  }
  for (let i = 0; i < 5; i++) {
    const start = nowNs();
    const checksum = kernel(n);
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

const n = parseInt(Deno.readTextFileSync(Deno.args[0]).trim(), 10);
console.log(Deno.args[1] === "hot" ? runHot(n) : kernel(n));
