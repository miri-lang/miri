// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=07795af817a233429eb9536c988cb16a9ecb0d09c9bd57d33c8269db0bcc7468

// ray-tracer: spheres shaded by a light, one ray per pixel (see ray-tracer.mi).

use std::time::Instant;

#[derive(Clone, Copy)]
struct Vec3 {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Clone, Copy)]
struct Sphere {
    center: Vec3,
    radius: f64,
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z }
}

fn scale(a: Vec3, k: f64) -> Vec3 {
    Vec3 { x: a.x * k, y: a.y * k, z: a.z * k }
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn normalize(a: Vec3) -> Vec3 {
    scale(a, 1.0 / dot(a, a).sqrt())
}

fn hit(sphere: Sphere, direction: Vec3) -> f64 {
    let oc = scale(sphere.center, -1.0);
    let b = dot(oc, direction);
    let c = dot(oc, oc) - sphere.radius * sphere.radius;
    let discriminant = b * b - c;
    if discriminant < 0.0 {
        return -1.0;
    }
    -b - discriminant.sqrt()
}

fn kernel(n: i64) -> String {
    let v = |x, y, z| Vec3 { x, y, z };
    let spheres = [
        Sphere { center: v(0.0, 0.0, 5.0), radius: 1.0 },
        Sphere { center: v(1.5, 0.5, 6.0), radius: 1.0 },
        Sphere { center: v(-1.5, -0.5, 4.5), radius: 0.75 },
        Sphere { center: v(0.0, -101.0, 5.0), radius: 100.0 },
    ];
    let light = normalize(v(-1.0, 1.0, -1.0));
    let (mut hash, mut hits): (i64, i64) = (0, 0);
    for py in 0..n {
        for px in 0..n {
            let direction = normalize(v(px as f64 / n as f64 - 0.5, 0.5 - py as f64 / n as f64, 1.0));
            let mut nearest = 1000000.0;
            let mut shade = 0;
            for sphere in &spheres {
                let t = hit(*sphere, direction);
                if t > 0.0 && t < nearest {
                    nearest = t;
                    let normal = normalize(sub(scale(direction, t), sphere.center));
                    let lambert = dot(normal, light);
                    shade = if lambert > 0.0 { (lambert * 255.0) as i64 } else { 0 };
                }
            }
            if nearest < 1000000.0 {
                hits += 1;
            }
            hash = (hash * 31 + shade) % 1000000007;
        }
    }
    format!("{hash} {hits}")
}

fn run_hot(n: i64) -> String {
    let expected = kernel(n);
    let mut agree = true;
    for _ in 1..5 {
        let checksum = kernel(n);
        agree = agree && checksum == expected;
    }
    for _ in 0..5 {
        let start = Instant::now();
        let checksum = kernel(n);
        let elapsed = start.elapsed().as_nanos();
        eprintln!("hot_ns={elapsed}");
        agree = agree && checksum == expected;
    }
    if !agree {
        eprintln!("checksum differs between iterations");
        std::process::exit(1);
    }
    expected
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).unwrap_or_else(|_| std::process::exit(2));
    let input: i64 = text.trim().parse().unwrap_or_else(|_| std::process::exit(2));
    let hot = args.get(2).is_some_and(|a| a == "hot");
    println!("{}", if hot { run_hot(input) } else { kernel(input) });
}
