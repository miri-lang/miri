// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=d4a597b5e4bc7126f07b419d14d1acb78275f58f916d2864803265e239e8c30e

// ray-tracer: spheres shaded by a light, one ray per pixel (see ray-tracer.mi).

interface Vec3 {
  x: number;
  y: number;
  z: number;
}

interface Sphere {
  center: Vec3;
  radius: number;
}

const vec = (x: number, y: number, z: number): Vec3 => ({ x, y, z });
const sub = (a: Vec3, b: Vec3): Vec3 => vec(a.x - b.x, a.y - b.y, a.z - b.z);
const scale = (a: Vec3, k: number): Vec3 => vec(a.x * k, a.y * k, a.z * k);
const dot = (a: Vec3, b: Vec3): number => a.x * b.x + a.y * b.y + a.z * b.z;
const normalize = (a: Vec3): Vec3 => scale(a, 1.0 / Math.sqrt(dot(a, a)));

function hit(sphere: Sphere, direction: Vec3): number {
  const oc = scale(sphere.center, -1.0);
  const b = dot(oc, direction);
  const c = dot(oc, oc) - sphere.radius * sphere.radius;
  const discriminant = b * b - c;
  if (discriminant < 0.0) {
    return -1.0;
  }
  return -b - Math.sqrt(discriminant);
}

function kernel(n: number): string {
  const spheres: Sphere[] = [
    { center: vec(0.0, 0.0, 5.0), radius: 1.0 },
    { center: vec(1.5, 0.5, 6.0), radius: 1.0 },
    { center: vec(-1.5, -0.5, 4.5), radius: 0.75 },
    { center: vec(0.0, -101.0, 5.0), radius: 100.0 },
  ];
  const light = normalize(vec(-1.0, 1.0, -1.0));
  let hash = 0;
  let hits = 0;
  for (let py = 0; py < n; py++) {
    for (let px = 0; px < n; px++) {
      const direction = normalize(vec(px / n - 0.5, 0.5 - py / n, 1.0));
      let nearest = 1000000.0;
      let shade = 0;
      for (const sphere of spheres) {
        const t = hit(sphere, direction);
        if (t > 0.0 && t < nearest) {
          nearest = t;
          const normal = normalize(sub(scale(direction, t), sphere.center));
          const lambert = dot(normal, light);
          shade = lambert > 0.0 ? Math.trunc(lambert * 255.0) : 0;
        }
      }
      if (nearest < 1000000.0) {
        hits += 1;
      }
      hash = (hash * 31 + shade) % 1000000007;
    }
  }
  return `${hash} ${hits}`;
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
