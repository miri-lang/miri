// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=0bb45df7f3733da618adb388a75858ffc41b6a6f6d95ccc135480b623d69de1e

// mandelbrot: the Mandelbrot set packed as a bitmap (see mandelbrot.mi).

use std::time::Instant;

fn stays_bounded(cr: f64, ci: f64) -> bool {
    let (mut zr, mut zi) = (0.0, 0.0);
    for _ in 0..50 {
        let tr = zr * zr - zi * zi + cr;
        zi = 2.0 * zr * zi + ci;
        zr = tr;
        if zr * zr + zi * zi > 4.0 {
            return false;
        }
    }
    true
}

fn kernel(n: i64) -> String {
    let mut hash: i64 = 0;
    let mut inside: i64 = 0;
    for y in 0..n {
        let ci = 2.0 * y as f64 / n as f64 - 1.0;
        let mut byte: i64 = 0;
        let mut bits = 0;
        for x in 0..n {
            let cr = 2.0 * x as f64 / n as f64 - 1.5;
            byte *= 2;
            if stays_bounded(cr, ci) {
                byte += 1;
                inside += 1;
            }
            bits += 1;
            if bits == 8 {
                hash = (hash * 31 + byte) % 1000000007;
                byte = 0;
                bits = 0;
            }
        }
        if bits > 0 {
            byte <<= 8 - bits;
            hash = (hash * 31 + byte) % 1000000007;
        }
    }
    format!("{hash} {inside}")
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
    let text = std::fs::read_to_string(&args[1]).expect("readable input");
    let n: i64 = text.trim().parse().expect("an integer parameter");
    let hot = args.get(2).is_some_and(|a| a == "hot");
    println!("{}", if hot { run_hot(n) } else { kernel(n) });
}
